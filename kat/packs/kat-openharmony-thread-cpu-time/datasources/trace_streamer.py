from __future__ import annotations

from collections.abc import Mapping
from datetime import datetime
from decimal import Decimal
import math
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
    description="以只读 SQL 查询 Thread CPU Time 使用的 Trace Streamer SQLite 数据库。",
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

        arrays: list[pa.Array] = []
        for index, field in enumerate(schema):
            arrays.append(_result_array(rows, index=index, field=field))
        return dp.Table.from_arrow(pa.Table.from_arrays(arrays, schema=schema))


def _result_array(
    rows: list[tuple[object | None, ...]],
    *,
    index: int,
    field: pa.Field,
) -> pa.Array:
    values = [row[index] for row in rows]
    expected_types = _expected_python_types(field.type)
    if expected_types is not None:
        expected = " or ".join(expected_type.__name__ for expected_type in expected_types)
        for value in values:
            if value is not None and type(value) not in expected_types:
                raise TypeError(
                    f"Trace Streamer result column {field.name!r} must have exact "
                    f"type {expected}, got {type(value).__name__}"
                )
    try:
        result = pa.array(values, type=field.type)
    except TypeError as error:
        raise TypeError(
            f"Trace Streamer result column {field.name!r} cannot be represented "
            f"as {field.type}: {error}"
        ) from error
    except (ValueError, OverflowError) as error:
        raise ValueError(
            f"Trace Streamer result column {field.name!r} cannot be represented "
            f"as {field.type}: {error}"
        ) from error

    if pa.types.is_floating(field.type):
        for source, encoded in zip(values, result.to_pylist(), strict=True):
            if (
                source is not None
                and math.isfinite(source)
                and not math.isfinite(encoded)
            ):
                raise ValueError(
                    f"Trace Streamer result column {field.name!r} overflows "
                    f"{field.type}"
                )
    return result


def _expected_python_types(
    data_type: pa.DataType,
) -> tuple[type[object], ...] | None:
    if pa.types.is_boolean(data_type):
        return (bool,)
    if pa.types.is_integer(data_type):
        return (int,)
    if pa.types.is_floating(data_type):
        return (float,)
    if (
        pa.types.is_string(data_type)
        or pa.types.is_large_string(data_type)
        or _is_string_view(data_type)
    ):
        return (str,)
    if pa.types.is_binary(data_type) or pa.types.is_large_binary(data_type):
        return (bytes,)
    if pa.types.is_timestamp(data_type):
        return (datetime, kat.WallClockTimestamp)
    if pa.types.is_decimal128(data_type) or pa.types.is_decimal256(data_type):
        return (Decimal,)
    return None


def _is_string_view(data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, "is_string_view", None)
    return bool(predicate and predicate(data_type))


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
