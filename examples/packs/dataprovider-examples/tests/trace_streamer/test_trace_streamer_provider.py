from pathlib import Path
import sqlite3
import sys

import pytest

from kat import dataprovider as dp
from kat.pack.datasources import trace_streamer as trace_streamer_module
from kat.pack.datasources.trace_streamer import TraceStreamerProvider


def _successful_trace_streamer(tmp_path: Path) -> tuple[Path, Path]:
    source = tmp_path / "fixture.htrace"
    source.write_text(
        """
import sqlite3
from pathlib import Path
import sys

if sys.argv[1] != "-e":
    raise SystemExit(2)
database = Path(sys.argv[2])
if database.parent != Path.cwd():
    raise SystemExit(3)
connection = sqlite3.connect(database)
connection.execute(
    "CREATE TABLE native_hook(event_type TEXT NOT NULL, heap_size INTEGER NOT NULL)"
)
connection.executemany(
    "INSERT INTO native_hook VALUES (?, ?)",
    [("AllocEvent", 7), ("AllocEvent", 11), ("FreeEvent", 5)],
)
connection.commit()
connection.close()
""".strip(),
        encoding="utf-8",
    )
    return Path(sys.executable), source


@pytest.mark.parametrize(
    "field",
    ("source", "executable", "workspace"),
)
def test_provider_locations_must_be_paths(tmp_path: Path, field: str):
    arguments = {
        "source": tmp_path / "input.htrace",
        "executable": tmp_path / "trace_streamer",
        "workspace": tmp_path / "workspace",
    }
    arguments[field] = str(arguments[field])

    with pytest.raises(TypeError, match=rf"{field}.*Path"):
        TraceStreamerProvider(**arguments)


def test_query_before_decode_is_rejected(tmp_path: Path):
    provider = TraceStreamerProvider(
        source=tmp_path / "input.htrace",
        executable=tmp_path / "trace_streamer",
        workspace=tmp_path / "workspace",
    )

    assert type(provider) is TraceStreamerProvider
    assert not hasattr(provider, "context")
    with pytest.raises(RuntimeError, match="decode"):
        provider.query("SELECT 1 AS value", schema={"value": int})


def test_decode_then_query_returns_a_reusable_eager_table(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    )

    decoded = provider.decode()
    first = decoded.query(
        """
        SELECT event_type, COUNT(*) AS event_count, SUM(heap_size) AS heap_size
        FROM native_hook
        GROUP BY event_type
        ORDER BY event_type
        """,
        schema={"event_type": str, "event_count": int, "heap_size": int},
    )
    second = provider.query(
        "SELECT event_type, heap_size FROM native_hook ORDER BY rowid",
        schema={"event_type": str, "heap_size": int},
    )

    assert decoded is provider
    assert type(first) is dp.Table
    assert first.to_rows() == [
        {"event_type": "AllocEvent", "event_count": 2, "heap_size": 18},
        {"event_type": "FreeEvent", "event_count": 1, "heap_size": 5},
    ]
    assert first.to_rows() == [
        {"event_type": "AllocEvent", "event_count": 2, "heap_size": 18},
        {"event_type": "FreeEvent", "event_count": 1, "heap_size": 5},
    ]
    assert second.to_rows() == [
        {"event_type": "AllocEvent", "heap_size": 7},
        {"event_type": "AllocEvent", "heap_size": 11},
        {"event_type": "FreeEvent", "heap_size": 5},
    ]


def test_failed_redecode_discards_old_and_current_workspaces(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    workspace = tmp_path / "workspace"
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=workspace,
    ).decode()
    assert tuple(workspace.iterdir()) == (workspace / "trace.db",)
    source.write_text("raise SystemExit(9)", encoding="utf-8")

    with pytest.raises(RuntimeError, match="decode failed"):
        provider.decode()

    assert not workspace.exists()
    with pytest.raises(RuntimeError, match="decode"):
        provider.query("SELECT 1 AS value", schema={"value": int})


def test_decode_removes_the_lexical_workspace_leaf(monkeypatch, tmp_path: Path):
    removed: list[Path] = []
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        trace_streamer_module,
        "_remove_owned_workspace",
        removed.append,
    )
    provider = TraceStreamerProvider(
        source=Path("missing.htrace"),
        executable=Path("trace_streamer"),
        workspace=Path("workspace"),
    )

    with pytest.raises(RuntimeError, match="source"):
        provider.decode()

    assert removed == [Path("workspace"), Path("workspace")]


def test_workspace_removal_failure_keeps_provider_unready_and_retries_cleanup(
    monkeypatch,
    tmp_path: Path,
):
    removed: list[Path] = []
    workspace = tmp_path / "workspace"

    def fail_removal(path: Path) -> None:
        removed.append(path)
        raise PermissionError("workspace is busy")

    monkeypatch.setattr(
        trace_streamer_module,
        "_remove_owned_workspace",
        fail_removal,
    )
    provider = TraceStreamerProvider(
        source=tmp_path / "input.htrace",
        executable=tmp_path / "trace_streamer",
        workspace=workspace,
    )

    with pytest.raises(RuntimeError, match="decode failed"):
        provider.decode()

    assert removed == [workspace, workspace]
    with pytest.raises(RuntimeError, match="decode"):
        provider.query("SELECT 1 AS value", schema={"value": int})


def test_new_provider_rebuilds_the_same_owned_workspace(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    workspace = tmp_path / "workspace"
    TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=workspace,
    ).decode()
    stale = workspace / "stale-sidecar"
    stale.write_text("stale", encoding="utf-8")
    source.write_text("raise SystemExit(9)", encoding="utf-8")

    replacement = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=workspace,
    )
    with pytest.raises(RuntimeError, match="decode failed"):
        replacement.decode()

    assert not workspace.exists()


@pytest.mark.parametrize(
    "program",
    [
        "pass",
        """
from pathlib import Path
import sys
Path(sys.argv[2]).write_bytes(b"not a sqlite database")
""",
        """
import sqlite3
import sys
connection = sqlite3.connect(sys.argv[2])
connection.close()
""",
    ],
    ids=("missing-db", "invalid-db", "no-relation"),
)
def test_decode_accepts_only_a_valid_sqlite_with_relations(
    tmp_path: Path,
    program: str,
):
    source = tmp_path / "fixture.htrace"
    source.write_text(program.strip(), encoding="utf-8")
    workspace = tmp_path / "workspace"
    provider = TraceStreamerProvider(
        source=source,
        executable=Path(sys.executable),
        workspace=workspace,
    )

    with pytest.raises(RuntimeError):
        provider.decode()

    assert not workspace.exists()
    with pytest.raises(RuntimeError, match="decode"):
        provider.query("SELECT 1 AS value", schema={"value": int})


def test_query_is_read_only_and_can_retry_after_failure(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    with pytest.raises(RuntimeError, match="query failed"):
        provider.query(
            "DELETE FROM native_hook RETURNING event_type",
            schema={"event_type": str},
        )

    result = provider.query(
        "SELECT COUNT(*) AS event_count FROM native_hook",
        schema={"event_count": int},
    )
    assert result.to_rows() == [{"event_count": 3}]


def test_query_rejects_attach_without_creating_a_file_and_can_retry(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()
    attached = tmp_path / "attached.db"

    with pytest.raises(RuntimeError, match="query failed"):
        provider.query(
            f"ATTACH DATABASE '{attached.as_posix()}' AS attached",
            schema={"value": int},
        )

    assert not attached.exists()
    assert provider.query(
        "SELECT COUNT(*) AS event_count FROM native_hook",
        schema={"event_count": int},
    ).to_rows() == [{"event_count": 3}]


def test_query_closes_a_connection_when_read_only_setup_fails(
    tmp_path: Path,
    monkeypatch,
):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    class FailingConnection:
        closed = False

        def execute(self, sql: str):
            raise sqlite3.OperationalError("setup failed")

        def close(self):
            self.closed = True

    connection = FailingConnection()
    monkeypatch.setattr(sqlite3, "connect", lambda *args, **kwargs: connection)

    with pytest.raises(RuntimeError, match="query failed"):
        provider.query("SELECT 1 AS value", schema={"value": int})

    assert connection.closed


@pytest.mark.parametrize("sql", (None, "", "   ", b"SELECT 1"))
def test_query_requires_non_empty_sql(tmp_path: Path, sql: object):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    with pytest.raises(TypeError, match="non-empty string"):
        provider.query(sql, schema={"value": int})


def test_query_schema_must_be_a_mapping(tmp_path: Path):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    with pytest.raises(TypeError, match="schema.*mapping"):
        provider.query("SELECT 1 AS value", schema=[("value", int)])


def test_invalid_query_schema_is_rejected_before_source_io(
    tmp_path: Path,
    monkeypatch,
):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    def unexpected_open(_database):
        raise AssertionError("invalid Schema must fail before opening SQLite")

    monkeypatch.setattr(
        trace_streamer_module,
        "_open_query_connection",
        unexpected_open,
    )

    with pytest.raises(TypeError, match="Datasource columns"):
        provider.query("SELECT 1 AS value", schema={"value": list})


def test_process_start_failure_is_a_clean_decode_failure(tmp_path: Path):
    source = tmp_path / "input.htrace"
    source.write_bytes(b"trace")
    executable = tmp_path / "not-an-executable"
    executable.write_text("not executable", encoding="utf-8")
    workspace = tmp_path / "workspace"
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=workspace,
    )

    with pytest.raises(RuntimeError, match="decode failed"):
        provider.decode()

    assert not workspace.exists()
    with pytest.raises(RuntimeError, match="decode"):
        provider.query("SELECT 1 AS value", schema={"value": int})


def test_query_enforces_schema_order_then_can_retry_with_parameters(
    tmp_path: Path,
):
    executable, source = _successful_trace_streamer(tmp_path)
    provider = TraceStreamerProvider(
        source=source,
        executable=executable,
        workspace=tmp_path / "workspace",
    ).decode()

    with pytest.raises(ValueError) as captured:
        provider.query(
            "SELECT event_type, heap_size FROM native_hook",
            schema={"heap_size": int, "event_type": str},
        )
    message = str(captured.value)
    assert "expected ('heap_size', 'event_type')" in message
    assert "got ('event_type', 'heap_size')" in message

    empty = provider.query(
        """
        SELECT event_type, heap_size
        FROM native_hook
        WHERE heap_size > :minimum
        """,
        schema={"event_type": str, "heap_size": int},
        params={"minimum": 100},
    )
    all_null = provider.query(
        "SELECT NULL AS note FROM native_hook LIMIT 1",
        schema={"note": str | None},
    )

    assert empty.columns == ("event_type", "heap_size")
    assert empty.to_rows() == []
    assert all_null.to_rows() == [{"note": None}]


def test_decode_resolves_relative_paths_before_switching_workspace(
    tmp_path: Path,
    monkeypatch,
):
    executable, source = _successful_trace_streamer(tmp_path)
    monkeypatch.chdir(tmp_path)
    provider = TraceStreamerProvider(
        source=Path(source.name),
        executable=executable,
        workspace=Path("workspace"),
    ).decode()

    result = provider.query(
        "SELECT COUNT(*) AS event_count FROM native_hook",
        schema={"event_count": int},
    )

    assert result.to_rows() == [{"event_count": 3}]
