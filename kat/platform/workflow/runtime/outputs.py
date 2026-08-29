from __future__ import annotations

import asyncio
import logging
from pathlib import Path
from typing import Any, NoReturn

import pyarrow.parquet as pq
from datafusion import DataFrame
from kat.datasource import Table
from kat._identifiers import valid_output_name

_LOGGER = logging.getLogger(__name__)


class OutputMaterializationError(Exception):
    """A public-safe failure while writing a private Run Output artifact."""


def materialize_outputs(
    value: object,
    candidate_path: Path,
) -> dict[str, dict[str, Any]]:
    try:
        outputs = _normalize_outputs(value)
    except (TypeError, ValueError) as error:
        raise OutputMaterializationError(str(error)) from error
    output_root = candidate_path / "outputs"
    if output_root.exists():
        raise OutputMaterializationError("Run Output directory already exists")
    try:
        output_root.mkdir()
    except OSError:
        _LOGGER.exception("failed to create the private Run Output directory")
        raise OutputMaterializationError(
            "Run Output directory could not be created"
        ) from None

    materialized: dict[str, dict[str, Any]] = {}
    for name in sorted(outputs):
        value = outputs[name]
        if isinstance(value, Table):
            materialized[name] = _write_table(
                value,
                output_root / f"{name}.parquet",
                name,
            )
        else:
            materialized[name] = asyncio.run(
                _write_output(
                    value,
                    output_root / f"{name}.parquet",
                    name,
                )
            )
    return materialized


def _normalize_outputs(value: object) -> dict[str, DataFrame | Table]:
    if isinstance(value, DataFrame):
        candidates: dict[object, object] = {"main": value}
    elif isinstance(value, Table):
        candidates = {"main": value}
    elif type(value) is dict:
        candidates = value
    else:
        raise TypeError(
            "Workflow must return a datasource.Table, DataFusion DataFrame, "
            "or a non-empty exact dict"
        )
    if not candidates:
        raise ValueError("Workflow must return at least one Table Output")

    outputs: dict[str, DataFrame | Table] = {}
    for name, relation in candidates.items():
        if type(name) is not str or not valid_output_name(name):
            raise ValueError(f"invalid Output name: {name!r}")
        if not isinstance(relation, (Table, DataFrame)):
            raise TypeError(
                f"Output {name!r} must be a datasource.Table or DataFusion DataFrame"
            )
        outputs[name] = relation
    return outputs


def _write_table(
    table: Table, output_path: Path, output_name: str
) -> dict[str, Any]:
    arrow_table = table.to_arrow()
    try:
        pq.write_table(arrow_table, output_path, compression="zstd")
    except (Exception, SystemExit):
        _raise_output_write_error(output_name)
    return _output_metadata(arrow_table.schema, arrow_table.num_rows)


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
