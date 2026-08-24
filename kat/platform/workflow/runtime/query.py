from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta
import math
from pathlib import Path
from typing import Any

import pyarrow as pa
from datafusion import SQLOptions, SessionContext
from datafusion.catalog import Schema

from .request import QueryDatasetRequest, QueryRunRequest
from .sources import _PRIVATE_DEFAULT_CATALOG, open_source_operation


_READ_ONLY = (
    SQLOptions()
    .with_allow_ddl(False)
    .with_allow_dml(False)
    .with_allow_statements(False)
)
_NANOSECONDS_PER_SECOND = 1_000_000_000
_UNIX_EPOCH = datetime(1970, 1, 1)


@dataclass(frozen=True)
class QueryRunRuntimeResult:
    columns: list[dict[str, str]]
    rows: list[list[object]]


def query_run(request: QueryRunRequest) -> QueryRunRuntimeResult:
    with open_source_operation(
        current_pack=None,
        dataset=request.dataset,
        pack_search=request.pack_search,
        enable_url_table=True,
    ) as operation:
        _register_schema(
            operation.session,
            "output",
            {
                name: request.run_path / "outputs" / f"{name}.parquet"
                for name in request.outputs
            },
        )
        return _query(operation.session, request.sql)


def query_dataset(request: QueryDatasetRequest) -> QueryRunRuntimeResult:
    with open_source_operation(
        current_pack=None,
        dataset=request.dataset,
        pack_search=request.pack_search,
        enable_url_table=True,
    ) as operation:
        return _query(operation.session, request.sql)


def _query(session: SessionContext, sql: str) -> QueryRunRuntimeResult:
    frame = session.sql(sql, options=_READ_ONLY)
    schema = frame.schema()
    _validate_result_types(schema)
    return QueryRunRuntimeResult(
        columns=[{"name": field.name, "type": str(field.type)} for field in schema],
        rows=_collect_rows(frame),
    )


def _register_schema(
    session: SessionContext,
    name: str,
    tables: dict[str, Path],
) -> None:
    schema = Schema.memory_schema()
    for table_name, path in tables.items():
        schema.register_table(table_name, session.read_parquet(str(path)))
    session.catalog(_PRIVATE_DEFAULT_CATALOG).register_schema(name, schema)


def _collect_rows(frame: Any) -> list[list[object]]:
    rows: list[list[object]] = []
    for batch in frame.collect():
        for row_index in range(batch.num_rows):
            rows.append(
                [
                    _json_scalar(batch.column(column_index), row_index)
                    for column_index in range(batch.num_columns)
                ]
            )
    return rows


def _validate_result_types(schema: pa.Schema) -> None:
    for field in schema:
        data_type = field.type
        supported = (
            pa.types.is_null(data_type)
            or pa.types.is_boolean(data_type)
            or pa.types.is_integer(data_type)
            or pa.types.is_floating(data_type)
            or pa.types.is_decimal128(data_type)
            or pa.types.is_decimal256(data_type)
            or pa.types.is_string(data_type)
            or pa.types.is_large_string(data_type)
            or _is_string_view(data_type)
            or _is_utc_nanosecond_timestamp(data_type)
        )
        if not supported:
            raise TypeError(
                f"query result type {data_type} is not supported; explicitly project it "
                "to a supported scalar type in the PACK or SQL"
            )


def _json_scalar(array: pa.Array, index: int) -> Any:
    scalar = array[index]
    if not scalar.is_valid:
        return None
    data_type = array.type
    if pa.types.is_int64(data_type) or pa.types.is_uint64(data_type):
        return str(scalar.as_py())
    if pa.types.is_timestamp(data_type):
        return _format_utc_nanoseconds(scalar.value)
    value = scalar.as_py()
    if pa.types.is_decimal128(data_type) or pa.types.is_decimal256(data_type):
        return format(value, "f")
    if pa.types.is_floating(data_type) and not math.isfinite(value):
        raise ValueError(
            "query result contains a non-finite float; explicitly filter or project it"
        )
    return value


def _format_utc_nanoseconds(value: int) -> str:
    # Arrow timestamp[ns] is int64, so its full 1677–2262 range fits datetime.
    seconds, nanoseconds = divmod(value, _NANOSECONDS_PER_SECOND)
    instant = _UNIX_EPOCH + timedelta(seconds=seconds)
    rendered = (
        f"{instant.year:04d}-{instant.month:02d}-{instant.day:02d}T"
        f"{instant.hour:02d}:{instant.minute:02d}:{instant.second:02d}"
    )
    if nanoseconds:
        rendered += f".{nanoseconds:09d}".rstrip("0")
    return rendered + "Z"


def _is_string_view(data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, "is_string_view", None)
    return bool(predicate and predicate(data_type))


def _is_utc_nanosecond_timestamp(data_type: pa.DataType) -> bool:
    return (
        pa.types.is_timestamp(data_type)
        and data_type.unit == "ns"
        and data_type.tz == "UTC"
    )
