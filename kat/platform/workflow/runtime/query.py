from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Literal

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

_INLINE_MAX_ROWS = 100
_INLINE_MAX_BYTES = 65_536


@dataclass(frozen=True)
class InlineQueryRunRuntimeResult:
    format: Literal["json"] = field(init=False, default="json")
    columns: list[dict[str, str]]
    rows: list[dict[str, object]]


@dataclass(frozen=True)
class FileQueryRunRuntimeResult:
    format: Literal["ndjson"] = field(init=False, default="ndjson")
    columns: list[dict[str, str]]


type QueryRunRuntimeResult = InlineQueryRunRuntimeResult | FileQueryRunRuntimeResult


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
    columns = [{"name": field.name, "type": str(field.type)} for field in schema]
    if request.result_path.stat().st_size > _INLINE_MAX_BYTES:
        return FileQueryRunRuntimeResult(columns=columns)

    rows: list[dict[str, object]] = []
    with request.result_path.open("r", encoding="utf-8", newline="") as file:
        for line in file:
            row = json.loads(line, parse_constant=_reject_json_constant)
            if type(row) is not dict:
                raise ValueError("Query Result rows must be JSON objects")
            rows.append(row)
    if len(rows) > _INLINE_MAX_ROWS:
        return FileQueryRunRuntimeResult(columns=columns)
    return InlineQueryRunRuntimeResult(columns=columns, rows=rows)


def _reject_json_constant(value: str) -> object:
    raise ValueError(f"Query Result contains invalid JSON constant {value!r}")


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
