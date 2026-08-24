from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq


def write_materialized_source(
    dataset: Path,
    *,
    pack: str,
    source: str,
    tables: Mapping[str, pa.Table],
) -> dict[str, Path]:
    root = dataset / "sources" / pack / source / "tables"
    root.mkdir(parents=True, exist_ok=True)
    paths: dict[str, Path] = {}
    for name, table in sorted(tables.items()):
        path = (root / f"{name}.parquet").resolve()
        pq.write_table(table, path)
        paths[name] = path
    return paths


def materialized_dataset_request(
    dataset: Path,
    *,
    pack: str,
    source: str,
    tables: Mapping[str, Path],
) -> dict[str, object]:
    return {
        "path": str(dataset.resolve()),
        "sources": [
            {
                "pack": pack,
                "source": source,
                "kind": "materialized",
                "tables": [
                    {"name": name, "path": str(path.resolve())}
                    for name, path in sorted(tables.items())
                ],
            }
        ],
    }
