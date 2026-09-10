from pathlib import Path
import sqlite3
import sys

import pytest
from kat import dataprovider as dp

import pyarrow as pa

from kat.dataprovider.trace_streamer import TraceStreamerProvider


def test_init_decodes_and_reuses_published_source_without_parser(tmp_path: Path):
    source = tmp_path / "capture.htrace"
    source.write_text(
        "import sqlite3, sys\n"
        "assert sys.argv[1] == '-e'\n"
        "with sqlite3.connect(sys.argv[2]) as db:\n"
        "    db.execute('CREATE TABLE event(value INTEGER)')\n"
        "    db.execute('INSERT INTO event VALUES (42)')\n",
        encoding="utf-8",
    )
    root = tmp_path / "materializations"
    root.mkdir()
    first = TraceStreamerProvider(
        source=source, executable=Path(sys.executable), workspace_root=root
    )
    source.unlink()
    reused = TraceStreamerProvider(
        source=source, executable=tmp_path / "missing-parser", workspace_root=root
    )
    opened = TraceStreamerProvider(sqlite_path=str(root / "capture" / "trace.db"))
    for provider in (first, reused, opened):
        assert provider.query(
            "SELECT value FROM event WHERE value = :value",
            params={"value": 42},
            schema=pa.schema([("value", pa.int64())]),
        ).to_rows() == [{"value": 42}]


def _database(path: Path) -> Path:
    connection = sqlite3.connect(path)
    try:
        connection.execute("CREATE TABLE event(value INTEGER NOT NULL)")
        connection.commit()
    finally:
        connection.close()
    return path.resolve(strict=True)


def test_provider_requires_an_exact_absolute_regular_file(tmp_path: Path):
    database = _database(tmp_path / "trace.db")

    provider = TraceStreamerProvider(sqlite_path=str(database))

    assert type(provider) is TraceStreamerProvider
    with pytest.raises(ValueError, match="absolute"):
        TraceStreamerProvider(sqlite_path="trace.db")
    with pytest.raises(ValueError, match="exist"):
        TraceStreamerProvider(sqlite_path=str(tmp_path / "missing.db"))
    with pytest.raises(ValueError, match="regular file"):
        TraceStreamerProvider(sqlite_path=str(tmp_path))
    (database.parent / "child").mkdir()
    with pytest.raises(ValueError, match="exact"):
        TraceStreamerProvider(
            sqlite_path=str(database.parent / "child" / ".." / database.name)
        )


def test_query_binds_named_parameters_and_returns_a_physical_table(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    connection = sqlite3.connect(database)
    try:
        connection.executemany("INSERT INTO event VALUES (?)", [(1,), (2,), (3,)])
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerProvider(sqlite_path=str(database))

    result = provider.query(
        "SELECT value FROM event WHERE value >= :minimum ORDER BY value",
        schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        params={"minimum": 2},
    )

    assert type(result) is dp.Table
    assert result.to_rows() == [{"value": 2}, {"value": 3}]


def test_query_requires_a_named_mapping_and_exact_physical_schema(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    connection = sqlite3.connect(database)
    try:
        connection.execute("INSERT INTO event VALUES (128)")
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerProvider(sqlite_path=str(database))

    with pytest.raises(TypeError, match="named mapping"):
        provider.query(
            "SELECT value FROM event WHERE value = ?",
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
            params=[128],
        )
    with pytest.raises(ValueError, match="exactly match schema order"):
        provider.query(
            "SELECT value AS actual FROM event",
            schema=pa.schema([pa.field("expected", pa.int64(), nullable=False)]),
        )
    with pytest.raises(ValueError, match="column 'value'.*int8"):
        provider.query(
            "SELECT value FROM event",
            schema=pa.schema([pa.field("value", pa.int8(), nullable=False)]),
        )
    with pytest.raises(TypeError, match="column 'value'.*exact type int"):
        provider.query(
            "SELECT 1.5 AS value",
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        )
    with pytest.raises(TypeError, match="column 'value'.*exact type float"):
        provider.query(
            "SELECT 1 AS value",
            schema=pa.schema([pa.field("value", pa.float64(), nullable=False)]),
        )
    with pytest.raises(ValueError, match="column 'value'.*overflows float"):
        provider.query(
            "SELECT 1e40 AS value",
            schema=pa.schema([pa.field("value", pa.float32(), nullable=False)]),
        )


def test_query_preserves_the_declared_schema_for_empty_results_and_rejects_nulls(
    tmp_path: Path,
):
    database = _database(tmp_path / "trace.db")
    provider = TraceStreamerProvider(sqlite_path=str(database))
    schema = pa.schema(
        [pa.field("value", pa.int64(), nullable=False)],
        metadata={b"source": b"trace-streamer"},
    )

    result = provider.query(
        "SELECT value FROM event WHERE 0",
        schema=schema,
    )

    assert result.to_rows() == []
    assert result.to_arrow().schema.equals(schema, check_metadata=True)
    with pytest.raises(ValueError, match="column 'value'.*not nullable"):
        provider.query("SELECT NULL AS value", schema=schema)


def test_query_rejects_attach_ddl_dml_and_pragma(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    attached = tmp_path / "attached.db"
    provider = TraceStreamerProvider(sqlite_path=str(database))
    empty_schema = pa.schema([pa.field("value", pa.int64())])

    forbidden = [
        ("ATTACH DATABASE :path AS escaped", {"path": str(attached)}),
        ("CREATE TABLE escaped(value INTEGER)", {}),
        ("INSERT INTO event VALUES (1)", {}),
        ("PRAGMA user_version", {}),
    ]
    for sql, params in forbidden:
        with pytest.raises(sqlite3.DatabaseError, match="(?i)(authorized|readonly)"):
            provider.query(sql, schema=empty_schema, params=params)

    assert not attached.exists()
    assert provider.query(
        "SELECT COUNT(*) AS count FROM event", schema=pa.schema([("count", pa.int64())])
    ).to_rows() == [{"count": 0}]


@pytest.mark.parametrize(
    "arguments",
    [
        {},
        {"source": Path("x")},
        {"sqlite_path": "x", "source": Path("x")},
        {"sqlite_path": "x", "workspace_root": Path("x")},
        {"sqlite_path": "x", "executable": Path("x")},
    ],
)
def test_constructor_rejects_missing_or_mixed_inputs(arguments):
    with pytest.raises((TypeError, ValueError)):
        TraceStreamerProvider(**arguments)


@pytest.mark.parametrize(
    "body, message",
    [
        (
            "import sys; sys.stderr.write('parser detail'); sys.exit(7)",
            "exit 7.*parser detail",
        ),
        ("pass", "must exist"),
        (
            "from pathlib import Path; import sys; Path(sys.argv[2]).write_text('broken')",
            "database",
        ),
        ("import sqlite3, sys; sqlite3.connect(sys.argv[2]).close()", "no relations"),
        (
            "from pathlib import Path; import sys; Path(sys.argv[2]).mkdir()",
            "regular file",
        ),
    ],
)
def test_bad_decode_does_not_publish_or_leave_candidates(tmp_path, body, message):
    source = tmp_path / "capture.htrace"
    source.write_text(body, encoding="utf-8")
    root = tmp_path / "materializations"
    root.mkdir()
    with pytest.raises((RuntimeError, ValueError, sqlite3.Error), match=message):
        TraceStreamerProvider(
            source=source, executable=Path(sys.executable), workspace_root=root
        )
    assert list(root.iterdir()) == []


def test_corrupt_existing_materialization_is_preserved(tmp_path):
    destination = tmp_path / "capture"
    destination.mkdir()
    database = destination / "trace.db"
    database.write_bytes(b"broken SQLite")
    with pytest.raises(sqlite3.DatabaseError):
        TraceStreamerProvider(
            source=Path("capture.htrace"),
            executable=Path("missing"),
            workspace_root=tmp_path,
        )
    assert database.read_bytes() == b"broken SQLite"
    assert list(tmp_path.iterdir()) == [destination]


@pytest.mark.parametrize(
    "name",
    ["con.trace", "NUL.trace", "com1.trace", "lpt9.trace", "bad?.trace", "bad .trace"],
)
def test_invalid_source_stem_is_rejected_before_io(tmp_path, name):
    with pytest.raises(ValueError, match="source stem"):
        TraceStreamerProvider(
            source=Path(name), executable=Path("unused"), workspace_root=tmp_path
        )
    assert list(tmp_path.iterdir()) == []


def test_shared_materialization_rejects_linked_database(tmp_path):
    outside = _database(tmp_path / "outside.db")
    destination = tmp_path / "capture"
    destination.mkdir()
    try:
        (destination / "trace.db").symlink_to(outside)
    except OSError as error:
        pytest.skip(f"symlink creation unavailable: {error}")
    with pytest.raises(ValueError, match="regular file"):
        TraceStreamerProvider(
            source=Path("capture.htrace"),
            executable=Path("unused"),
            workspace_root=tmp_path,
        )
    assert outside.is_file()


def test_parallel_initialization_uses_one_complete_published_database(tmp_path):
    from concurrent.futures import ThreadPoolExecutor

    source = tmp_path / "capture.htrace"
    source.write_text(
        "import sqlite3, sys, time\n"
        "from pathlib import Path\n"
        "output = Path(sys.argv[2])\n"
        "(output.parent / 'ready').touch()\n"
        "deadline = time.monotonic() + 10\n"
        "while len(list(output.parents[2].glob('.trace-streamer-*/decoded/ready'))) < 2:\n"
        "    assert time.monotonic() < deadline, 'second decoder did not start'\n"
        "    time.sleep(0.01)\n"
        "time.sleep(0.1)\n"
        "with sqlite3.connect(output) as db:\n"
        "    db.execute('CREATE TABLE event(value INTEGER)')\n"
        "    db.execute('INSERT INTO event VALUES (73)')\n",
        encoding="utf-8",
    )
    root = tmp_path / "materializations"
    root.mkdir()

    def construct():
        return TraceStreamerProvider(
            source=source, executable=Path(sys.executable), workspace_root=root
        )

    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(construct) for _ in range(2)]
        providers = [future.result() for future in futures]
    for provider in providers:
        assert provider.query(
            "SELECT value FROM event", schema=pa.schema([("value", pa.int64())])
        ).to_rows() == [{"value": 73}]
    assert [path.name for path in root.iterdir()] == ["capture"]


def test_process_start_failure_cleans_its_candidate(tmp_path):
    source = tmp_path / "capture.htrace"
    source.write_bytes(b"trace")
    executable = tmp_path / "invalid-parser.exe"
    executable.write_bytes(b"not an executable")
    root = tmp_path / "materializations"
    root.mkdir()
    with pytest.raises(OSError):
        TraceStreamerProvider(source=source, executable=executable, workspace_root=root)
    assert list(root.iterdir()) == []
