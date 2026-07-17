from __future__ import annotations

import asyncio
import datetime as dt
import math
from pathlib import Path
import re
from typing import Any

import pyarrow as pa
from datafusion import SQLOptions, SessionContext, udf
from datafusion.catalog import Schema, SchemaProvider

from .execution import ClockResolver, _validate_clock_target_literals


QUERY_ROW_LIMIT = 1_000
QUERY_TIME_LIMIT_SECONDS = 5.0
_OUTPUT_ID = re.compile(r"[0-9a-f]{32}\Z")
_READ_ONLY = (
    SQLOptions()
    .with_allow_ddl(False)
    .with_allow_dml(False)
    .with_allow_statements(False)
)


class QueryLimitExceeded(ValueError):
    pass


class DatasetCapabilityError(ValueError):
    def __init__(self, dataset: dict[str, object], table_name: str) -> None:
        self.dataset = dataset
        self.table_name = table_name
        status = dataset["status"]
        if status == "unavailable":
            detail = (
                f"current Dataset at {dataset['path']!r} is unavailable: "
                f"{dataset['cause']}"
            )
        else:
            detail = "this Run did not provide a Dataset"
        super().__init__(
            f"DataFusion cannot resolve dataset.{table_name} because {detail}"
        )

    def help(self) -> str:
        if self.dataset["status"] == "unavailable":
            return (
                "Healthy output.* tables remain queryable; query only output.* or restore "
                "the current Dataset, then retry"
            )
        return (
            "Healthy output.* tables remain queryable; query only output.* or rerun the "
            "Workflow with a Dataset"
        )


class _UnavailableDatasetSchema(SchemaProvider):
    def __init__(self, dataset: dict[str, object]) -> None:
        self._dataset = dataset

    def table_names(self) -> set[str]:
        return set()

    def table(self, name: str) -> None:
        raise DatasetCapabilityError(self._dataset, name)

    def table_exist(self, name: str) -> bool:
        return True


def query_run(request: dict[str, object]) -> dict[str, object]:
    run_path = _run_path(request["run_path"])
    outputs = request["outputs"]
    dataset = request["dataset"]
    sql = request["sql"]
    if type(outputs) is not dict or not outputs:
        raise ValueError("query_run outputs must be a non-empty object")
    if type(sql) is not str or not sql.strip():
        raise ValueError("query_run sql must be a non-empty string")

    session = SessionContext()
    _register_schema(
        session,
        "output",
        {
            name: session.read_parquet(_output_path(run_path, output_id))
            for name, output_id in outputs.items()
        },
    )
    resolved_dataset = _query_dataset(dataset)
    if resolved_dataset is not None:
        _register_schema(
            session,
            "dataset",
            {
                name: session.read_parquet(path)
                for name, path in resolved_dataset["tables"].items()
            },
        )
    else:
        session.catalog().register_schema(
            "dataset", _UnavailableDatasetSchema(dataset)
        )
    resolver = ClockResolver(resolved_dataset, query_dataset=dataset)
    session.register_udf(
        udf(
            resolver.convert,
            [pa.string(), pa.uint64(), pa.string()],
            pa.uint64(),
            "stable",
            name="kat_convert_clock",
        )
    )
    frame = session.sql(sql, options=_READ_ONLY)
    _validate_clock_target_literals(frame)
    columns = [
        {"name": field.name, "type": str(field.type)} for field in frame.schema()
    ]
    _validate_result_types(frame.schema())
    try:
        rows = asyncio.run(_collect_rows(frame))
    except TimeoutError as error:
        raise QueryLimitExceeded(
            f"query execution time limit exceeded ({QUERY_TIME_LIMIT_SECONDS} seconds); "
            "narrow the projection, filter, aggregate, or use an explicit LIMIT"
        ) from error
    return {"columns": columns, "rows": rows}


def _run_path(value: object) -> Path:
    if type(value) is not str:
        raise TypeError("query_run run_path must be a string")
    path = Path(value)
    if not path.is_absolute():
        raise ValueError("query_run run_path must be absolute")
    resolved = path.resolve(strict=True)
    if resolved != path or not resolved.is_dir():
        raise ValueError("query_run run_path must be a canonical directory")
    return resolved


def _output_path(run_path: Path, value: object) -> Path:
    if type(value) is not str or _OUTPUT_ID.fullmatch(value) is None:
        raise ValueError("invalid opaque Output reference")
    output_entry = run_path / "outputs"
    if output_entry.is_symlink() or _is_junction(output_entry):
        raise ValueError("Run Output directory must not be a link")
    output_root = output_entry.resolve(strict=True)
    if not output_root.is_dir() or output_root.parent != run_path:
        raise ValueError("Run Output directory is missing or invalid")
    candidate = output_root / f"{value}.parquet"
    if candidate.is_symlink() or _is_junction(candidate):
        raise ValueError("Output reference must not be a link")
    resolved = candidate.resolve(strict=True)
    if not resolved.is_file() or resolved.parent != output_root:
        raise ValueError("Output reference resolves outside the Run")
    return resolved


def _query_dataset(value: object) -> dict[str, object] | None:
    if type(value) is not dict:
        raise TypeError("query_run dataset must be an object")
    status = value.get("status")
    if status == "not_provided":
        if set(value) != {"status"}:
            raise ValueError("not_provided Dataset must contain only status")
        return None
    if status == "unavailable":
        if set(value) != {"status", "path", "cause"}:
            raise ValueError("unavailable Dataset has an invalid field set")
        if any(type(value[name]) is not str or not value[name] for name in ("path", "cause")):
            raise TypeError("unavailable Dataset path and cause must be non-empty strings")
        return None
    if status != "available" or set(value) != {"status", "path", "tables"}:
        raise ValueError("available Dataset has an invalid field set")
    path = value["path"]
    tables = value["tables"]
    if type(path) is not str or not Path(path).is_absolute() or type(tables) is not dict:
        raise TypeError("available Dataset path and tables have invalid types")
    for name, table_path in tables.items():
        if type(name) is not str or type(table_path) is not str or not Path(table_path).is_absolute():
            raise TypeError("available Dataset table references must be absolute strings")
    return {"path": path, "tables": tables}


def _register_schema(
    session: SessionContext, name: str, frames: dict[str, object]
) -> None:
    schema = Schema.memory_schema(session)
    session.catalog().register_schema(name, schema)
    for table_name, frame in frames.items():
        schema.register_table(table_name, frame)


async def _collect_rows(frame: object) -> list[list[object]]:
    rows: list[list[object]] = []
    async with asyncio.timeout(QUERY_TIME_LIMIT_SECONDS):
        async for batch in frame.execute_stream():
            arrow_batch = batch.to_pyarrow()
            if len(rows) + arrow_batch.num_rows > QUERY_ROW_LIMIT:
                raise QueryLimitExceeded(
                    f"query row limit exceeded ({QUERY_ROW_LIMIT} rows); narrow the "
                    "projection, filter, aggregate, or use an explicit LIMIT"
                )
            for row_index in range(arrow_batch.num_rows):
                rows.append(
                    [
                        _json_scalar(arrow_batch.column(column_index), row_index)
                        for column_index in range(arrow_batch.num_columns)
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
        return _timestamp_ns_utc(scalar.value)
    if pa.types.is_decimal128(data_type) or pa.types.is_decimal256(data_type):
        return _decimal_fact(array, index)
    value = scalar.as_py()
    if pa.types.is_floating(data_type) and not math.isfinite(value):
        raise ValueError(
            "query result contains a non-finite float; explicitly filter or project it"
        )
    return value


def _decimal_fact(array: pa.Array, index: int) -> dict[str, object]:
    data_type = array.type
    bits = 128 if pa.types.is_decimal128(data_type) else 256
    width = bits // 8
    data_buffer = array.buffers()[1]
    offset = (array.offset + index) * width
    physical = data_buffer.slice(offset, width).to_pybytes()
    unscaled = int.from_bytes(physical, byteorder="little", signed=True)
    return {
        "decimal": {
            "bits": bits,
            "unscaled": str(unscaled),
            "precision": data_type.precision,
            "scale": data_type.scale,
        }
    }


def _is_string_view(data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, "is_string_view", None)
    return bool(predicate and predicate(data_type))


def _is_utc_nanosecond_timestamp(data_type: pa.DataType) -> bool:
    return (
        pa.types.is_timestamp(data_type)
        and data_type.unit == "ns"
        and data_type.tz == "UTC"
    )


def _timestamp_ns_utc(value: int) -> str:
    seconds, nanoseconds = divmod(value, 1_000_000_000)
    try:
        instant = dt.datetime.fromtimestamp(seconds, tz=dt.UTC)
    except (OverflowError, OSError, ValueError) as error:
        raise ValueError("UTC nanosecond timestamp is outside the supported RFC 3339 range") from error
    rendered = (
        f"{instant.year:04d}-{instant.month:02d}-{instant.day:02d}T"
        f"{instant.hour:02d}:{instant.minute:02d}:{instant.second:02d}"
    )
    if nanoseconds:
        rendered += f".{nanoseconds:09d}".rstrip("0")
    return rendered + "Z"


def _is_junction(path: Path) -> bool:
    check = getattr(path, "is_junction", None)
    return bool(check and check())
