from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def register_dataset(ctx: Any, dataset_path: Path) -> None:
    catalog_path = dataset_path / "catalog.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    for table in catalog["tables"]:
        name = table["name"]
        relative_path = Path(table["path"])
        if relative_path.is_absolute() or ".." in relative_path.parts:
            raise ValueError(f"dataset table {name} has invalid path: {table['path']}")
        parquet_path = dataset_path / relative_path
        ctx.register_parquet(name, str(parquet_path))
