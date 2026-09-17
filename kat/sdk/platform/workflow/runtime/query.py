from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

import pyarrow as pa
from datafusion import DataFrameWriteOptions, SQLOptions, SessionContext
from datafusion.catalog import Schema
from datafusion.expr import Explain

from .request import QueryRunRequest


_READ_ONLY = (
    SQLOptions()
    .with_allow_ddl(False)
    .with_allow_dml(False)
    .with_allow_statements(False)
)


@dataclass(frozen=True)
class QueryRunRuntimeResult:
    columns: list[dict[str, str]]


def query_run(request: QueryRunRequest) -> QueryRunRuntimeResult:
    session = SessionContext()
    _register_schema(session, "output", request.outputs)

    frame = session.sql(request.sql, options=_READ_ONLY)
    schema = frame.schema()
    _validate_unique_struct_field_names(schema)
    if isinstance(frame.logical_plan().to_variant(), Explain):
        # DataFusion rejects COPY TO directly above Explain because Explain must be
        # the plan root. Cache only this bounded diagnostic relation before writing.
        frame = frame.cache()
    frame.write_json(
        str(request.result_path),
        DataFrameWriteOptions(single_file_output=True),
    )
    return QueryRunRuntimeResult(
        columns=[{"name": field.name, "type": str(field.type)} for field in schema],
    )


def _register_schema(
    session: SessionContext,
    name: str,
    tables: dict[str, Path],
) -> None:
    schema = Schema.memory_schema()
    for table_name, path in tables.items():
        schema.register_table(table_name, session.read_parquet(str(path)))
    session.catalog().register_schema(name, schema)


def _validate_unique_struct_field_names(schema: pa.Schema) -> None:
    _validate_field_group(schema, "query result")


def _validate_field_group(fields: Iterable[pa.Field], location: str) -> None:
    seen: set[str] = set()
    for field in fields:
        if field.name in seen:
            raise ValueError(
                f"{location} sibling field names must be exactly unique; "
                f"duplicate name {field.name!r}"
            )
        seen.add(field.name)
        _validate_nested_type(field.type, f"{location}.{field.name}")


def _validate_nested_type(data_type: pa.DataType, location: str) -> None:
    if pa.types.is_struct(data_type):
        _validate_field_group(data_type, location)
        return

    for index in range(getattr(data_type, "num_fields", 0)):
        _validate_nested_type(data_type.field(index).type, location)

    for attribute in ("value_type", "storage_type"):
        nested = getattr(data_type, attribute, None)
        if isinstance(nested, pa.DataType):
            _validate_nested_type(nested, location)
