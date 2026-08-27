from __future__ import annotations

import asyncio
import logging
import os
from pathlib import Path
from typing import Any, NoReturn

import kat
import pyarrow.parquet as pq
from datafusion import DataFrame
from kat._identifiers import valid_output_name

from .datasource import WorkflowOperation


_LOGGER = logging.getLogger(__name__)


class OutputMaterializationError(Exception):
    """A public-safe failure while writing a private Run Output artifact."""


def materialize_outputs(
    value: object,
    operation: WorkflowOperation,
) -> dict[str, dict[str, Any]]:
    try:
        outputs, published_tables = _normalize_outputs(value, operation)
    except (TypeError, ValueError) as error:
        raise OutputMaterializationError(str(error)) from error
    output_root = operation.output_root
    try:
        output_root.mkdir(exist_ok=True)
        junction = getattr(output_root, "is_junction", None)
        if (
            not output_root.is_dir()
            or output_root.is_symlink()
            or (junction is not None and junction())
        ):
            raise OSError("Run Output path is not an ordinary directory")
    except OSError:
        _LOGGER.exception("failed to create the private Run Output directory")
        raise OutputMaterializationError(
            "Run Output directory could not be created"
        ) from None

    for name, value in outputs.items():
        path = output_root / f"{name}.parquet"
        if isinstance(value, DataFrame) and os.path.lexists(path):
            raise OutputMaterializationError(
                f"Output {name!r} backing already exists"
            )

    materialized: dict[str, dict[str, Any]] = {}
    for name in sorted(outputs):
        value = outputs[name]
        if isinstance(value, kat.Table):
            schema, row_count = operation.table_facts(value)
            materialized[name] = _output_metadata(schema, row_count)
        else:
            materialized[name] = asyncio.run(
                _write_output(
                    value,
                    output_root / f"{name}.parquet",
                    name,
                )
            )
    operation.mark_published_tables(published_tables)
    return materialized


def _normalize_outputs(
    value: object,
    operation: WorkflowOperation,
) -> tuple[dict[str, DataFrame | kat.Table], set[str]]:
    if isinstance(value, DataFrame):
        candidates: dict[object, object] = {"main": value}
    elif isinstance(value, kat.Table):
        candidates = {value.name: value}
    elif type(value) is dict:
        candidates = value
    else:
        raise TypeError(
            "Workflow must return a DataFusion DataFrame, KAT Table, or a non-empty named mapping"
        )
    if not candidates:
        raise ValueError("Workflow must return at least one Table Output")

    outputs: dict[str, DataFrame | kat.Table] = {}
    published_tables: set[str] = set()
    dataframe_names: set[str] = set()
    for name, relation in candidates.items():
        if type(name) is not str or not valid_output_name(name):
            raise ValueError(f"invalid Output name: {name!r}")
        if isinstance(relation, kat.Table):
            if name != relation.name:
                raise ValueError(
                    f"Table Output key {name!r} must equal Table.name {relation.name!r}"
                )
            operation.table_facts(relation)
            published_tables.add(name)
        elif isinstance(relation, DataFrame):
            dataframe_names.add(name)
        else:
            raise TypeError(
                f"Output {name!r} must be a DataFusion DataFrame or KAT Table"
            )
        outputs[name] = relation
    conflicts = sorted(dataframe_names & operation.provider_names)
    if conflicts:
        raise ValueError(
            "DataFrame Output names conflict with Provider Tables: "
            + ", ".join(conflicts)
        )
    return outputs, published_tables


async def _write_output(
    frame: DataFrame, output_path: Path, output_name: str
) -> dict[str, Any]:
    schema = frame.schema()
    row_count = 0
    try:
        writer = pq.ParquetWriter(output_path, schema, compression="zstd")
    except (Exception, SystemExit):
        _raise_output_write_error(output_name)

    try:
        async for batch in frame.execute_stream():
            arrow_batch = batch.to_pyarrow().cast(schema)
            try:
                writer.write_batch(arrow_batch)
            except (Exception, SystemExit):
                _raise_output_write_error(output_name)
            row_count += arrow_batch.num_rows
    except (Exception, SystemExit):
        try:
            writer.close()
        except (Exception, SystemExit):
            _LOGGER.exception(
                "failed to close private Run Output %r after an earlier failure",
                output_name,
            )
        raise
    try:
        writer.close()
    except (Exception, SystemExit):
        _raise_output_write_error(output_name)
    return _output_metadata(schema, row_count)


def _output_metadata(schema: Any, row_count: int) -> dict[str, Any]:
    return {
        "columns": [
            {"name": field.name, "type": str(field.type)} for field in schema
        ],
        "row_count": row_count,
    }


def _raise_output_write_error(output_name: str) -> NoReturn:
    _LOGGER.exception("failed to write private Run Output %r", output_name)
    raise OutputMaterializationError(
        f"Output {output_name!r} could not be materialized"
    ) from None
