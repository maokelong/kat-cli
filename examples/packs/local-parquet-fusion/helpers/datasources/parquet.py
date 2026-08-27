from __future__ import annotations

from collections.abc import Iterator, Mapping
from contextlib import contextmanager
from datetime import datetime
from decimal import Decimal
import math
from pathlib import Path
import re

from datafusion import DataFrame, SQLOptions, SessionContext
import kat
import pyarrow as pa


_DURATION = re.compile(r"([0-9]+(?:\.[0-9]{1,9})?)(ns|us|ms|s|min|h)\Z")
_DURATION_FACTORS = {
    "ns": 1,
    "us": 1_000,
    "ms": 1_000_000,
    "s": 1_000_000_000,
    "min": 60_000_000_000,
    "h": 3_600_000_000_000,
}


def provider(
    ctx: kat.Context,
    *,
    tables: Mapping[str, Path],
) -> kat.Provider:
    """Create a KAT Provider backed only by the explicitly mapped Parquet tables."""
    return ctx.provider(LocalParquetExecutor(tables))


class LocalParquetExecutor:
    """PACK-owned local source executor with a private DataFusion catalog."""

    def __init__(self, tables: Mapping[str, Path]) -> None:
        if not isinstance(tables, Mapping):
            raise TypeError("tables must be a Mapping[str, pathlib.Path]")
        selected = dict(tables)
        if any(type(name) is not str or not name for name in selected):
            raise TypeError("table names must be non-empty strings")
        if any(not isinstance(path, Path) for path in selected.values()):
            raise TypeError("table paths must be pathlib.Path values")

        self._tables = selected
        self._session: SessionContext | None = None
        self._closed = False
        self._sql_options = (
            SQLOptions()
            .with_allow_ddl(False)
            .with_allow_dml(False)
            .with_allow_statements(False)
        )

    @contextmanager
    def execute(
        self,
        sql: str,
        params: object | None,
        *,
        scratch: Path,
    ) -> Iterator[pa.RecordBatchReader]:
        del scratch
        if self._closed:
            raise RuntimeError("LocalParquetExecutor is closed")
        session = self._session
        if session is None:
            session = SessionContext()
            try:
                for name, path in self._tables.items():
                    session.register_parquet(name, str(path))
            except (Exception, SystemExit):
                del session
                raise
            self._session = session
        if params is None:
            values: dict[str, object] = {}
        elif isinstance(params, Mapping):
            values = {
                _parameter_name(name): _sql_parameter(name, value)
                for name, value in params.items()
            }
        else:
            raise TypeError("local Parquet query params must be a Mapping or None")

        frame = session.sql(
            sql,
            options=self._sql_options,
            param_values=values,
        )
        reader = _record_batch_reader(frame)
        try:
            yield reader
        finally:
            reader.close()

    def close(self) -> None:
        self._closed = True
        self._session = None


def _parameter_name(name: object) -> str:
    if type(name) is not str or not name:
        raise TypeError("local Parquet query parameter names must be non-empty strings")
    return name


def _record_batch_reader(frame: DataFrame) -> pa.RecordBatchReader:
    # 锁定版 DataFusion/PyArrow 的 C stream 在 Parquet JOIN 首批会阻塞；
    # execute_stream 配合惰性 generator 保持同一逐 batch Reader 合同。
    stream = frame.execute_stream()
    return pa.RecordBatchReader.from_batches(
        frame.schema(),
        (batch.to_pyarrow() for batch in stream),
    )


def _sql_parameter(name: str, value: object) -> object:
    if type(value) is bool or type(value) is str:
        return value
    if type(value) is int and -(2**63) <= value < 2**63:
        return value
    if type(value) is float and math.isfinite(value):
        return value
    if isinstance(value, kat.Duration):
        return _duration_nanoseconds(str(value))
    if isinstance(value, kat.WallClockTimestamp):
        return pa.scalar(
            _wall_clock_nanoseconds(str(value)),
            type=pa.timestamp("ns", tz="UTC"),
        )
    raise TypeError(
        f"SQL parameter {name!r} must be bool, int64, finite float, str, "
        "Duration, or WallClockTimestamp"
    )


def _duration_nanoseconds(value: str) -> int:
    match = _DURATION.fullmatch(value)
    if match is None:
        raise ValueError(f"invalid Duration literal: {value!r}")
    nanoseconds = Decimal(match.group(1)) * _DURATION_FACTORS[match.group(2)]
    if nanoseconds != nanoseconds.to_integral_value() or not 0 <= nanoseconds < 2**63:
        raise ValueError(
            "Duration is not an exact non-negative int64 nanosecond value: "
            f"{value!r}"
        )
    return int(nanoseconds)


def _wall_clock_nanoseconds(value: str) -> int:
    base, _, fraction = value[:-1].partition(".")
    instant = datetime.fromisoformat(base)
    delta = instant - datetime(1970, 1, 1)
    seconds = delta.days * 86_400 + delta.seconds
    return seconds * 1_000_000_000 + int(fraction.ljust(9, "0") or "0")
