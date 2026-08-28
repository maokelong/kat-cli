from __future__ import annotations

from collections.abc import Mapping
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
from ._table import Table, from_arrow


_SQL_NAME = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")
_READ_ONLY = (
    SQLOptions()
    .with_allow_ddl(False)
    .with_allow_dml(False)
    .with_allow_statements(False)
)


def execute_sql(
    session: SessionContext,
    sql: str,
    *,
    params: Mapping[str, object] | None,
) -> Table:
    if type(sql) is not str or not sql.strip():
        raise TypeError("SQL must be a non-empty string")
    values = parameter_values(params)
    frame = session.sql(sql, options=_READ_ONLY, param_values=values)
    batches = frame.collect()
    return from_arrow(pa.Table.from_batches(batches, schema=frame.schema()))


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
    if type(value) is bool or type(value) is str:
        return value
    if type(value) is int and -(2**63) <= value < 2**63:
        return value
    if type(value) is float and math.isfinite(value):
        return value
    if isinstance(value, Duration):
        return _duration_nanoseconds(str(value))
    if isinstance(value, WallClockTimestamp):
        return pa.scalar(
            _wall_clock_nanoseconds(str(value)),
            type=pa.timestamp("ns", tz="UTC"),
        )
    raise TypeError(
        f"SQL parameter {name!r} must be bool, int64, finite float, str, "
        "Duration, or WallClockTimestamp"
    )
