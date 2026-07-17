from __future__ import annotations

import asyncio
from pathlib import Path
import re
import secrets
from typing import Any

import pyarrow.parquet as pq
from datafusion import DataFrame


_OUTPUT_NAME = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")


def publish_outputs(value: object, run_path: Path) -> dict[str, dict[str, Any]]:
    outputs = _normalize_outputs(value)
    output_root = run_path / "outputs"
    if output_root.exists():
        raise ValueError("Run Output directory already exists")
    output_root.mkdir()

    published: dict[str, dict[str, Any]] = {}
    issued: set[str] = set()
    for name in sorted(outputs):
        output_id = _new_output_id(output_root, issued)
        issued.add(output_id)
        published[name] = asyncio.run(
            _write_output(outputs[name], output_root / f"{output_id}.parquet", output_id)
        )
    return published


def _normalize_outputs(value: object) -> dict[str, DataFrame]:
    if isinstance(value, DataFrame):
        candidates: dict[object, object] = {"main": value}
    elif type(value) is dict:
        candidates = value
    else:
        raise TypeError(
            "Workflow must return a DataFusion DataFrame or a non-empty named DataFrame mapping"
        )
    if not candidates:
        raise ValueError("Workflow must return at least one Table Output")

    outputs: dict[str, DataFrame] = {}
    for name, frame in candidates.items():
        if type(name) is not str or _OUTPUT_NAME.fullmatch(name) is None:
            raise ValueError(f"invalid Output name: {name!r}")
        if not isinstance(frame, DataFrame):
            raise TypeError(f"Output {name!r} must be a DataFusion DataFrame")
        outputs[name] = frame
    return outputs


def _new_output_id(output_root: Path, issued: set[str]) -> str:
    while True:
        output_id = secrets.token_hex(16)
        if output_id not in issued and not (output_root / f"{output_id}.parquet").exists():
            return output_id


async def _write_output(
    frame: DataFrame, output_path: Path, output_id: str
) -> dict[str, Any]:
    schema = frame.schema()
    row_count = 0
    with pq.ParquetWriter(output_path, schema, compression="zstd") as writer:
        async for batch in frame.execute_stream():
            arrow_batch = batch.to_pyarrow().cast(schema)
            writer.write_batch(arrow_batch)
            row_count += arrow_batch.num_rows
    return {
        "output_id": output_id,
        "columns": [
            {"name": field.name, "type": str(field.type)} for field in schema
        ],
        "row_count": row_count,
    }
