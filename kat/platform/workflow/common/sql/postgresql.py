"""Execute PostgreSQL SQL and expose its rowset as a Workflow DataFrame."""

from __future__ import annotations

from collections.abc import Mapping
from decimal import Decimal
import os

from datafusion import DataFrame
import psycopg
import pyarrow as pa

from kat import Context


_ARROW_TYPES_BY_POSTGRESQL_OID: dict[int, pa.DataType] = {
    16: pa.bool_(),
    17: pa.binary(),
    19: pa.string(),
    20: pa.int64(),
    21: pa.int16(),
    23: pa.int32(),
    25: pa.string(),
    700: pa.float32(),
    701: pa.float64(),
    1042: pa.string(),
    1043: pa.string(),
    1082: pa.date32(),
    1083: pa.time64("us"),
    1114: pa.timestamp("us"),
    1184: pa.timestamp("us", tz="UTC"),
}
_NUMERIC_OID = 1700


def execute_sql_file(
    ctx: Context,
    sql_file_path: str | os.PathLike[str],
    parameters: Mapping[str, object] | None = None,
) -> DataFrame:
    """Execute an absolute UTF-8 PostgreSQL SQL file and return its rowset."""
    path = os.fspath(sql_file_path)
    if isinstance(path, bytes):
        raise TypeError("sql_file_path must resolve to a text path, not bytes")
    if not os.path.isabs(path):
        raise ValueError("sql_file_path must be an absolute path")
    with open(path, encoding="utf-8-sig", errors="strict") as sql_file:
        sql_text = sql_file.read()
    return execute_sql_text(ctx, sql_text, parameters)


def execute_sql_text(
    ctx: Context,
    sql_text: str,
    parameters: Mapping[str, object] | None = None,
) -> DataFrame:
    """Execute PostgreSQL SQL text and return its single rowset."""
    if type(sql_text) is not str:
        raise TypeError("sql_text must be a string")
    if parameters is not None and not isinstance(parameters, Mapping):
        raise TypeError("parameters must be a mapping with string keys")
    if parameters is not None and any(
        type(name) is not str for name in parameters
    ):
        raise TypeError("parameters must be a mapping with string keys")
    with psycopg.connect(autocommit=True) as connection:
        with connection.cursor() as cursor:
            cursor.execute(sql_text, parameters)
            rowsets = [
                (result.description, result.fetchall())
                for result in cursor.results()
                if result.description is not None
            ]
            if len(rowsets) != 1:
                raise ValueError("PostgreSQL SQL must produce exactly one rowset")
            description, rows = rowsets[0]
            assert description is not None
            names = [column.name for column in description]
            if not names:
                raise ValueError("PostgreSQL rowset must contain at least one column")
            if any(type(name) is not str or not name for name in names):
                raise ValueError("PostgreSQL rowset column names must be non-empty strings")
            if len(set(names)) != len(names):
                raise ValueError("PostgreSQL rowset column names must be unique")
            arrays = []
            for index, column in enumerate(description):
                arrow_type = _arrow_type(column)
                values = [row[index] for row in rows]
                if column.type_code == _NUMERIC_OID and any(
                    value is not None
                    and (
                        not isinstance(value, Decimal)
                        or not value.is_finite()
                    )
                    for value in values
                ):
                    raise ValueError(
                        f"unsupported PostgreSQL numeric value for column "
                        f"{column.name!r}: expected finite Decimal values"
                    )
                arrays.append(pa.array(values, type=arrow_type))
            table = pa.Table.from_arrays(arrays, names=names)
            return ctx.from_arrow(table)


def _arrow_type(column: object) -> pa.DataType:
    type_code = column.type_code
    if type_code == _NUMERIC_OID:
        precision = column.precision
        scale = column.scale
        if (
            type(precision) is not int
            or type(scale) is not int
            or not 1 <= precision <= 38
            or not 0 <= scale <= precision
        ):
            raise ValueError(
                f"unsupported PostgreSQL numeric type for column "
                f"{column.name!r}: precision={precision!r}, scale={scale!r}"
            )
        return pa.decimal128(precision, scale)

    arrow_type = _ARROW_TYPES_BY_POSTGRESQL_OID.get(type_code)
    if arrow_type is None:
        type_display = getattr(column, "type_display", None)
        raise ValueError(
            f"unsupported PostgreSQL type for column {column.name!r}: "
            f"{type_display or f'OID {type_code}'}"
        )
    return arrow_type


__all__ = ["execute_sql_file", "execute_sql_text"]
