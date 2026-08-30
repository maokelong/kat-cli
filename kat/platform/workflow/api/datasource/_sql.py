from __future__ import annotations

from collections.abc import Mapping
from datetime import datetime
from decimal import Decimal
import math
import re

from datafusion import SQLOptions, SessionContext
import pyarrow as pa

from .._temporal import (
    Duration,
    WallClockTimestamp,
    _duration_nanoseconds,
    _wall_clock_nanoseconds,
)
from ._schema import _datetime_nanoseconds
from ._table import Table


_SQL_NAME = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")
_READ_ONLY = (
    SQLOptions()
    .with_allow_ddl(False)
    .with_allow_dml(False)
    .with_allow_statements(False)
)
_INT64_MIN = -(2**63)
_INT64_MAX = 2**63 - 1


def prepare_query(
    sql: str,
    params: Mapping[str, object] | None,
) -> tuple[str, dict[str, object]]:
    values = parameter_values(params)
    if type(sql) is not str or not sql.strip():
        raise TypeError("SQL must be a non-empty string")
    return sql, values


def execute_sql(
    session: SessionContext,
    sql: str,
    *,
    values: Mapping[str, object],
) -> Table:
    frame = session.sql(sql, options=_READ_ONLY, param_values=dict(values))
    planned_schema = frame.schema()

    # collect() 前先按计划结果 Schema 准入，使宽 Catalog 投影无需扫描数据页
    # 即可失败。
    Table.from_arrow(pa.Table.from_batches([], schema=planned_schema))

    batches = frame.collect()
    arrow_table = pa.Table.from_batches(batches, schema=planned_schema)
    return Table.from_arrow(arrow_table)


def parameter_values(params: Mapping[str, object] | None) -> dict[str, object]:
    if params is None:
        return {}
    if not isinstance(params, Mapping):
        raise TypeError("SQL params must be a mapping or None")
    snapshot = dict(params.items())
    result: dict[str, object] = {}
    for name, value in snapshot.items():
        require_sql_name(name, "SQL parameter")
        result[name] = _sql_scalar(name, value)
    return result


def require_sql_name(name: object, label: str) -> str:
    if type(name) is not str or _SQL_NAME.fullmatch(name) is None:
        raise ValueError(f"invalid {label} name: {name!r}")
    return name


def _sql_scalar(name: str, value: object) -> object:
    if type(value) is bool:
        return pa.scalar(value, type=pa.bool_())
    if type(value) is int and _INT64_MIN <= value <= _INT64_MAX:
        return pa.scalar(value, type=pa.int64())
    if type(value) is float and math.isfinite(value):
        return pa.scalar(value, type=pa.float64())
    if type(value) is str:
        return pa.scalar(value, type=pa.string())
    if type(value) is bytes:
        return pa.scalar(value, type=pa.binary())
    if type(value) is datetime:
        return pa.scalar(
            _datetime_nanoseconds(value, location=f"SQL parameter {name!r}"),
            type=pa.timestamp("ns", tz="UTC"),
        )
    if type(value) is WallClockTimestamp:
        return pa.scalar(
            _wall_clock_nanoseconds(str(value)),
            type=pa.timestamp("ns", tz="UTC"),
        )
    if type(value) is Decimal:
        return _decimal_scalar(value, name=name)
    if type(value) is Duration:
        return pa.scalar(_duration_nanoseconds(str(value)), type=pa.int64())
    raise TypeError(
        f"SQL parameter {name!r} must be bool, int64, finite float, str, bytes, "
        "aware datetime, WallClockTimestamp, finite Decimal, or Duration"
    )


def _decimal_scalar(value: Decimal, *, name: str) -> pa.Decimal128Scalar | pa.Decimal256Scalar:
    if not value.is_finite():
        raise ValueError(f"SQL parameter {name!r} must be a finite Decimal")

    sign, raw_digits, exponent = value.as_tuple()
    digits = list(raw_digits)
    if not any(digits):
        normalized = Decimal((sign, (0,), 0))
        return pa.scalar(normalized, type=pa.decimal128(1, 0))

    while exponent < 0 and digits[-1] == 0:
        digits.pop()
        exponent += 1

    if exponent >= 0:
        precision = len(digits) + exponent
        scale = 0
        if precision > 76:
            raise ValueError(f"SQL parameter {name!r} exceeds Decimal256 precision")
        normalized_digits = (*digits, *((0,) * exponent))
    else:
        scale = -exponent
        precision = max(len(digits), scale)
        normalized_digits = tuple(digits)

    if precision > 76:
        raise ValueError(f"SQL parameter {name!r} exceeds Decimal256 precision")
    decimal_type = (
        pa.decimal128(precision, scale)
        if precision <= 38
        else pa.decimal256(precision, scale)
    )
    normalized = Decimal((sign, normalized_digits, -scale))
    return pa.scalar(normalized, type=decimal_type)
