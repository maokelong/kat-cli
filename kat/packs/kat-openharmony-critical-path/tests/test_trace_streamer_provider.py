from pathlib import Path
import sqlite3

import pyarrow as pa
import pytest

from kat import dataprovider as dp
from kat.pack.datasources.trace_streamer import TraceStreamerSQLiteProvider
from kat.pack.helpers.critical_path import TraceStreamerFacts


def _database(path: Path) -> Path:
    connection = sqlite3.connect(path)
    try:
        connection.execute("CREATE TABLE event(value INTEGER NOT NULL)")
        connection.commit()
    finally:
        connection.close()
    return path.resolve(strict=True)


def test_provider_declares_inspection_metadata():
    declaration = TraceStreamerSQLiteProvider.__kat_provider__

    assert declaration.name == "trace-streamer-sqlite"
    assert declaration.description
    guide = Path(__file__).parents[1] / "knowledge" / declaration.guide
    assert guide.is_file()
    assert guide.read_text(encoding="utf-8").startswith("# Trace Streamer SQLite Provider")


def test_provider_requires_an_exact_absolute_regular_file(tmp_path: Path):
    database = _database(tmp_path / "trace.db")

    assert type(
        TraceStreamerSQLiteProvider(sqlite_path=str(database))
    ) is TraceStreamerSQLiteProvider
    with pytest.raises(ValueError, match="absolute"):
        TraceStreamerSQLiteProvider(sqlite_path="trace.db")
    with pytest.raises(ValueError, match="exist"):
        TraceStreamerSQLiteProvider(sqlite_path=str(tmp_path / "missing.db"))
    with pytest.raises(ValueError, match="regular file"):
        TraceStreamerSQLiteProvider(sqlite_path=str(tmp_path))
    with pytest.raises(ValueError, match="exact"):
        TraceStreamerSQLiteProvider(
            sqlite_path=str(database.parent / "child" / ".." / database.name)
        )


def test_query_requires_named_parameters_and_an_exact_physical_schema(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    connection = sqlite3.connect(database)
    try:
        connection.executemany("INSERT INTO event VALUES (?)", [(2,), (128,)])
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))

    result = provider.query(
        "SELECT value FROM event WHERE value = :value",
        schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
        params={"value": 2},
    )

    assert type(result) is dp.Table
    assert result.to_rows() == [{"value": 2}]
    with pytest.raises(TypeError, match="named mapping"):
        provider.query(
            "SELECT value FROM event WHERE value = ?",
            schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
            params=[2],
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
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))
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
    with pytest.raises(ValueError, match="non-nullable column 'value'"):
        provider.query("SELECT NULL AS value", schema=schema)


def test_query_rejects_attach_ddl_dml_and_pragma(tmp_path: Path):
    database = _database(tmp_path / "trace.db")
    attached = tmp_path / "attached.db"
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))
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


def test_first_frame_uses_sqlite_named_parameters(tmp_path: Path):
    database = (tmp_path / "critical.db").resolve()
    connection = sqlite3.connect(database)
    try:
        connection.execute("CREATE TABLE process(ipid INT, pid INT, name TEXT)")
        connection.execute(
            "CREATE TABLE frame_slice("
            "id INT, itid INT, ts INT, dur INT, callstack_id INT, ipid INT, type INT)"
        )
        connection.execute(
            "CREATE TABLE thread(itid INT, ipid INT, tid INT, name TEXT)"
        )
        connection.execute(
            "CREATE TABLE sched_slice(ts INT, dur INT, cpu INT, priority INT, itid INT)"
        )
        connection.execute(
            "CREATE TABLE callstack("
            "id INT, parent_id INT, depth INT, ts INT, dur INT, name TEXT, callid INT)"
        )
        connection.execute(
            "CREATE TABLE instant("
            "wakeup_from INT, ref_type TEXT, ref INT, name TEXT, ts INT)"
        )
        connection.execute(
            "CREATE TABLE thread_state("
            "itid INT, ts INT, dur INT, state TEXT, cpu INT, arg_setid INT)"
        )
        connection.execute(
            "CREATE TABLE args(argset INT, key INT, value INT, datatype INT)"
        )
        connection.execute("CREATE TABLE data_dict(id INT, data TEXT)")
        connection.execute(
            "INSERT INTO process VALUES (10, 1000, :name)",
            {"name": "demo' --"},
        )
        connection.executemany(
            "INSERT INTO frame_slice VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                (1, 1, 100, 100, None, 10, 0),
                (2, 2, 150, 10, 7, 10, 0),
            ],
        )
        connection.execute("INSERT INTO thread VALUES (2, 10, 22, 'render')")
        connection.execute("INSERT INTO sched_slice VALUES (150, 10, 3, 120, 2)")
        connection.execute(
            "INSERT INTO callstack VALUES (7, NULL, 0, 150, 10, 'RenderFrame', 2)"
        )
        connection.execute(
            "INSERT INTO instant VALUES (9, 'itid', 2, 'sched_wakeup', 160)"
        )
        connection.execute(
            "INSERT INTO thread_state VALUES (2, 150, 10, 'Running', 3, 100)"
        )
        connection.executemany(
            "INSERT INTO data_dict VALUES (?, ?)",
            [(1, "iowait"), (2, "caller"), (3, "RenderFrame")],
        )
        connection.executemany(
            "INSERT INTO args VALUES (?, ?, ?, ?)",
            [(100, 1, 1, 0), (100, 2, 3, 1)],
        )
        connection.commit()
    finally:
        connection.close()
    provider = TraceStreamerSQLiteProvider(sqlite_path=str(database))

    facts = TraceStreamerFacts(provider)
    selected = facts.first_frame("demo' --")

    assert selected == {
        "frame_id": 2,
        "itid": 2,
        "ts": 150,
        "dur": 10,
        "callstack_id": 7,
        "ipid": 10,
        "pid": 1000,
        "process_name": "demo' --",
    }
    assert facts.metadata(2) == {
        "itid": 2,
        "ipid": 10,
        "tid": 22,
        "thread_name": "render",
        "pid": 1000,
        "process_name": "demo' --",
    }
    assert facts.sched(2, 150, 160) == [
        {"ts": 150, "dur": 10, "cpu": 3, "priority": 120}
    ]
    assert facts.callstacks(2, 150, 160) == [
        {
            "id": 7,
            "parent_id": None,
            "depth": 0,
            "ts": 150,
            "dur": 10,
            "function_name": "RenderFrame",
        }
    ]
    assert facts.waker(2, 160) == 9
    assert facts.states(2, 150, 160) == [
        {
            "start": 150,
            "end": 160,
            "state": "Running",
            "io_wait": 1,
            "blocked_function": "RenderFrame",
        }
    ]


def test_locate_workflow_reads_an_explicit_sqlite_path(kat_run, tmp_path: Path):
    database = (tmp_path / "locate.db").resolve()
    connection = sqlite3.connect(database)
    try:
        connection.execute("CREATE TABLE process(ipid INT, pid INT, name TEXT)")
        connection.execute(
            "CREATE TABLE frame_slice("
            "id INT, itid INT, ts INT, dur INT, callstack_id INT, ipid INT, type INT)"
        )
        connection.execute(
            "CREATE TABLE thread(itid INT, ipid INT, tid INT, name TEXT)"
        )
        connection.execute("INSERT INTO process VALUES (10, 1000, '.demo')")
        connection.execute(
            "INSERT INTO frame_slice VALUES (1, 2, 150, 10, 7, 10, 0)"
        )
        connection.execute("INSERT INTO thread VALUES (2, 10, 22, 'render')")
        connection.commit()
    finally:
        connection.close()

    output = kat_run(
        workflow="locate-first-actual-frame",
        arguments=[
            "--sqlite-path",
            str(database),
            "--process-name",
            ".demo",
        ],
    )["frame_window"]

    assert output.to_pylist() == [
        {
            "frame_id": 1,
            "root_itid": 2,
            "start_ts": 150,
            "end_ts": 160,
            "duration_ns": 10,
            "process_id": 1000,
            "process_name": ".demo",
            "thread_id": 22,
            "thread_name": "render",
            "callstack_id": 7,
            "clock_domain": "boottime",
        }
    ]


def test_extract_workflow_reads_an_explicit_sqlite_path(kat_run, tmp_path: Path):
    database = (tmp_path / "extract.db").resolve()
    connection = sqlite3.connect(database)
    try:
        connection.execute("CREATE TABLE process(ipid INT, pid INT, name TEXT)")
        connection.execute(
            "CREATE TABLE thread(itid INT, ipid INT, tid INT, name TEXT)"
        )
        connection.execute(
            "CREATE TABLE thread_state("
            "itid INT, ts INT, dur INT, state TEXT, cpu INT, arg_setid INT)"
        )
        connection.execute(
            "CREATE TABLE sched_slice(ts INT, dur INT, cpu INT, priority INT, itid INT)"
        )
        connection.execute(
            "CREATE TABLE callstack("
            "id INT, parent_id INT, depth INT, ts INT, dur INT, name TEXT, callid INT)"
        )
        connection.execute(
            "CREATE TABLE instant("
            "wakeup_from INT, ref_type TEXT, ref INT, name TEXT, ts INT)"
        )
        connection.execute(
            "CREATE TABLE args(argset INT, key INT, value INT, datatype INT)"
        )
        connection.execute("CREATE TABLE data_dict(id INT, data TEXT)")
        connection.execute("INSERT INTO process VALUES (10, 1000, '.demo')")
        connection.execute("INSERT INTO thread VALUES (1, 10, 11, 'render')")
        connection.execute(
            "INSERT INTO thread_state VALUES (1, 100, 10, 'Running', 3, NULL)"
        )
        connection.execute("INSERT INTO sched_slice VALUES (100, 10, 3, 120, 1)")
        connection.commit()
    finally:
        connection.close()

    outputs = kat_run(
        workflow="extract-critical-path",
        arguments=[
            "--sqlite-path",
            str(database),
            "--root-itid",
            "1",
            "--start-ts",
            "100",
            "--end-ts",
            "110",
        ],
    )

    assert set(outputs) == {
        "critical_path_segments",
        "critical_path_callstack_evidence",
    }
    assert outputs["critical_path_segments"].to_pylist()[0] == {
        "segment_id": 0,
        "parent_segment_id": None,
        "depth": 0,
        "clock_domain": "boottime",
        "start_ts": 100,
        "end_ts": 110,
        "duration_ns": 10,
        "itid": 1,
        "tid": 11,
        "thread_name": "render",
        "pid": 1000,
        "process_name": ".demo",
        "thread_state": "Running",
        "segment_kind": "execution",
        "relation_to_parent": "root",
        "cpu": 3,
        "priority": 120,
        "io_wait": None,
        "blocked_function": None,
        "termination_reason": None,
        "uncertainty_reason": "missing_callstack_evidence",
    }
    assert outputs["critical_path_callstack_evidence"].to_pylist() == []
