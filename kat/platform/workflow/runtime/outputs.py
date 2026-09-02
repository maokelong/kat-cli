from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, NoReturn

import pyarrow as pa
from kat.dataprovider import Table
from kat.dataprovider._parquet_writer import _ParquetRelationWriter
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
        materialized[name] = _write_table(
            outputs[name],
            output_root / f"{name}.parquet",
            name,
        )
    return materialized


def _normalize_outputs(value: object) -> dict[str, Table]:
    if type(value) is Table:
        candidates = {"main": value}
    elif type(value) is dict:
        candidates = value
    else:
        raise TypeError(
            "Workflow must return an exact dataprovider.Table or a non-empty "
            "exact dict"
        )
    if not candidates:
        raise ValueError("Workflow must return at least one Table Output")

    outputs: dict[str, Table] = {}
    for name, relation in candidates.items():
        if type(name) is not str or not valid_output_name(name):
            raise ValueError(f"invalid Output name: {name!r}")
        if type(relation) is not Table:
            raise TypeError(
                f"Output {name!r} must be an exact dataprovider.Table"
            )
        outputs[name] = relation
    return outputs


def _write_table(
    table: Table, output_path: Path, output_name: str
) -> dict[str, Any]:
    arrow_table = table.to_arrow()
    writer: _ParquetRelationWriter | None = None
    try:
        writer = _ParquetRelationWriter(
            output_path,
            arrow_table.schema,
            compression="zstd",
        )
        for batch in arrow_table.to_batches():
            if batch.num_rows == 0:
                continue
            writer.write_table(
                pa.Table.from_batches([batch], schema=arrow_table.schema),
                row_group_size=batch.num_rows,
            )
        metadata = writer.close()
    except (Exception, SystemExit) as error:
        if writer is not None:
            try:
                writer.close()
            except (Exception, SystemExit) as close_error:
                if close_error is not error:
                    error.add_note(
                        "Run Output writer also failed while closing: "
                        f"{type(close_error).__name__}: {close_error}"
                    )
        _raise_output_write_error(output_name)
    return _output_metadata(metadata.schema, metadata.row_count)


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
