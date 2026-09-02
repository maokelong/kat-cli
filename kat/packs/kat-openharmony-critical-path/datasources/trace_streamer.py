from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
import sqlite3

import pyarrow as pa

import kat
from kat import dataprovider as dp


_READ_ONLY_SQLITE_ACTIONS = frozenset(
    {
        sqlite3.SQLITE_FUNCTION,
        sqlite3.SQLITE_READ,
        sqlite3.SQLITE_RECURSIVE,
        sqlite3.SQLITE_SELECT,
    }
)
@kat.provider(
    name="trace-streamer-sqlite",
    description="以只读 SQL 查询 Critical Path 使用的 Trace Streamer SQLite 数据库。",
    guide="providers/trace-streamer-sqlite.md",
)
class TraceStreamerSQLiteProvider:
    """PACK-owned read-only access to one exact Trace Streamer database."""

    def __init__(self, *, sqlite_path: str) -> None:
        if type(sqlite_path) is not str:
            raise TypeError("Trace Streamer SQLite path must be a string")
        supplied = Path(sqlite_path)
        if not supplied.is_absolute():
            raise ValueError("Trace Streamer SQLite path must be absolute")
        try:
            resolved = supplied.resolve(strict=True)
        except (OSError, RuntimeError):
            raise ValueError("Trace Streamer SQLite path must exist") from None
        if resolved != supplied:
            raise ValueError("Trace Streamer SQLite path must identify its exact file")
        if not resolved.is_file():
            raise ValueError("Trace Streamer SQLite path must identify a regular file")
        self._database = resolved

    def query(
        self,
        sql: str,
        *,
        schema: pa.Schema,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        if type(sql) is not str or not sql.strip():
            raise TypeError("Trace Streamer SQL must be a non-empty string")
        if not isinstance(schema, pa.Schema):
            raise TypeError("Trace Streamer query schema must be a PyArrow schema")
        if params is None:
            bound: dict[str, object] = {}
        elif isinstance(params, Mapping) and all(
            type(name) is str for name in params
        ):
            bound = dict(params.items())
        else:
            raise TypeError("Trace Streamer query parameters must be a named mapping")

        connection = sqlite3.connect(
            f"{self._database.as_uri()}?mode=ro",
            uri=True,
        )
        try:
            pragma = connection.execute("PRAGMA query_only = ON")
            pragma.close()
            connection.set_authorizer(_authorize_read_only)
            cursor = connection.execute(sql, bound)
            try:
                actual_columns = tuple(
                    column[0] for column in (cursor.description or ())
                )
                expected_columns = tuple(schema.names)
                if actual_columns != expected_columns:
                    raise ValueError(
                        "Trace Streamer query columns must exactly match schema order: "
                        f"expected {expected_columns!r}, got {actual_columns!r}"
                    )
                rows = cursor.fetchall()
            finally:
                cursor.close()
        finally:
            connection.close()

        return dp.Table.from_rows(
            (
                dict(zip(expected_columns, row, strict=True))
                for row in rows
            ),
            schema=schema,
        )


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
