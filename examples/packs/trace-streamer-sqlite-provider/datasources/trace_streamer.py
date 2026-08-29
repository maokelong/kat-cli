from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
import shutil
import sqlite3
import subprocess

from kat import datasource as ds


NATIVE_HOOK_SUMMARY_SQL = """
SELECT
    event_type,
    COUNT(*) AS event_count,
    COALESCE(SUM(heap_size), 0) AS total_heap_size
FROM native_hook
WHERE event_type IS NOT NULL
GROUP BY event_type
ORDER BY event_type
"""

NATIVE_HOOK_SUMMARY_SCHEMA = {
    "event_type": str,
    "event_count": int,
    "total_heap_size": int,
}

_READ_ONLY_SQLITE_ACTIONS = frozenset(
    {
        sqlite3.SQLITE_FUNCTION,
        sqlite3.SQLITE_READ,
        sqlite3.SQLITE_RECURSIVE,
        sqlite3.SQLITE_SELECT,
    }
)


class TraceStreamerProvider:
    """PACK 自有、通过 Trace Streamer 物化 SQLite 的普通 Provider。"""

    def __init__(
        self,
        *,
        source: Path,
        executable: Path,
        materialization_root: Path,
    ) -> None:
        for field, value in (
            ("source", source),
            ("executable", executable),
            ("materialization_root", materialization_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Trace Streamer {field} must be a Path")
        self._source = source
        self._executable = executable
        self._materialization_root = materialization_root
        self._workspace: Path | None = None
        self._database: Path | None = None

    def decode(self) -> TraceStreamerProvider:
        """用 Trace Streamer 生成并校验本次 Provider 私有的 SQLite。"""
        self._discard_workspace()
        materialization_root = self._materialization_root.resolve(strict=False)
        workspace = materialization_root / "workspace"
        _remove_owned_workspace(workspace)
        if not self._source.is_file():
            raise RuntimeError("Trace Streamer source must be an existing file")
        if not self._executable.is_file():
            raise RuntimeError("Trace Streamer executable must be an existing file")
        source = self._source.resolve(strict=True)
        executable = self._executable.resolve(strict=True)

        materialization_root.mkdir(parents=True, exist_ok=True)
        workspace.mkdir()
        workspace = workspace.resolve(strict=True)
        self._workspace = workspace
        database = workspace / "trace.db"
        try:
            completed = subprocess.run(
                [
                    str(executable),
                    str(source),
                    "-e",
                    str(database),
                ],
                cwd=workspace,
                shell=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            if completed.returncode != 0:
                raise RuntimeError("Trace Streamer decode failed")
            if not database.is_file() or database.is_symlink():
                raise RuntimeError(
                    "Trace Streamer did not produce a regular SQLite file"
                )
            try:
                _verify_database(database)
            except sqlite3.Error:
                raise RuntimeError(
                    "Trace Streamer produced an invalid SQLite database"
                ) from None
        except OSError:
            self._discard_workspace()
            raise RuntimeError("Trace Streamer decode failed") from None
        except Exception:
            self._discard_workspace()
            raise
        self._database = database
        return self

    def _discard_workspace(self) -> None:
        self._database = None
        workspace = self._workspace
        self._workspace = None
        if workspace is not None:
            _remove_owned_workspace(workspace)

    def query(
        self,
        sql: str,
        *,
        schema: Mapping[str, object],
        params: object | None = None,
    ) -> ds.Table:
        if self._database is None:
            raise RuntimeError("Trace Streamer Provider must decode before query")
        if type(sql) is not str or not sql.strip():
            raise TypeError("Trace Streamer SQL must be a non-empty string")
        if not isinstance(schema, Mapping):
            raise TypeError("Trace Streamer query schema must be a mapping")

        try:
            connection = _open_query_connection(self._database)
            try:
                query = connection.cursor()
                try:
                    if params is None:
                        query.execute(sql)
                    else:
                        query.execute(sql, params)
                    actual_columns = tuple(
                        column[0] for column in (query.description or ())
                    )
                    expected_columns = tuple(schema)
                    if actual_columns != expected_columns:
                        raise ValueError(
                            "Trace Streamer query columns must exactly match "
                            f"schema order: expected {expected_columns!r}, "
                            f"got {actual_columns!r}"
                        )
                    rows = query.fetchall()
                finally:
                    query.close()
            finally:
                connection.close()
        except sqlite3.Error:
            raise RuntimeError("Trace Streamer query failed") from None

        return ds.table(schema=schema, rows=rows)


def _open_read_only(database: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(
        f"{database.resolve(strict=True).as_uri()}?mode=ro",
        uri=True,
    )
    try:
        pragma = connection.execute("PRAGMA query_only = ON")
        pragma.close()
    except BaseException:
        connection.close()
        raise
    return connection


def _open_query_connection(database: Path) -> sqlite3.Connection:
    connection = _open_read_only(database)
    try:
        connection.set_authorizer(_authorize_read_only)
    except BaseException:
        connection.close()
        raise
    return connection


def _authorize_read_only(
    action_code: int,
    first: str | None,
    second: str | None,
    database: str | None,
    trigger: str | None,
) -> int:
    del first, second, database, trigger
    if action_code in _READ_ONLY_SQLITE_ACTIONS:
        return sqlite3.SQLITE_OK
    return sqlite3.SQLITE_DENY


def _remove_owned_workspace(workspace: Path) -> None:
    if workspace.is_symlink() or workspace.is_file():
        workspace.unlink()
    elif workspace.exists():
        shutil.rmtree(workspace)


def _verify_database(database: Path) -> None:
    connection = _open_read_only(database)
    try:
        check = connection.execute("PRAGMA quick_check")
        try:
            if check.fetchone() != ("ok",):
                raise RuntimeError("Trace Streamer SQLite integrity check failed")
        finally:
            check.close()

        relations = connection.execute(
            """
            SELECT 1
            FROM sqlite_schema
            WHERE type IN ('table', 'view')
              AND name NOT LIKE 'sqlite_%'
            LIMIT 1
            """
        )
        try:
            if relations.fetchone() is None:
                raise RuntimeError("Trace Streamer SQLite contains no relations")
        finally:
            relations.close()
    finally:
        connection.close()
