from __future__ import annotations

import json
from pathlib import Path

from datafusion import SessionContext


def register_dataset(ctx: SessionContext, dataset_path: Path) -> None:
    dataset_root = dataset_path.resolve(strict=True)
    catalog_path = dataset_root / "catalog.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    if not isinstance(catalog, dict):
        raise TypeError("dataset catalog must be a JSON object")
    tables = catalog.get("tables")
    if not isinstance(tables, list):
        raise TypeError("dataset catalog tables must be a list")

    seen_names: set[str] = set()
    validated: list[tuple[str, Path]] = []
    for index, table in enumerate(tables):
        if not isinstance(table, dict):
            raise TypeError(f"dataset table at index {index} must be an object")
        name = table.get("name")
        path_text = table.get("path")
        if not isinstance(name, str) or not name:
            raise ValueError(f"dataset table at index {index} requires a non-empty name")
        if not isinstance(path_text, str) or not path_text:
            raise ValueError(f"dataset table {name!r} requires a non-empty path")
        if name in seen_names:
            raise ValueError(f"duplicate dataset table name: {name}")
        seen_names.add(name)

        relative_path = Path(path_text)
        if (
            relative_path.is_absolute()
            or relative_path.drive
            or relative_path.root
            or ".." in relative_path.parts
        ):
            raise ValueError(f"dataset table {name} has invalid relative path: {path_text}")
        candidate = dataset_root / relative_path
        if not candidate.exists():
            raise FileNotFoundError(f"dataset table {name} does not exist: {path_text}")
        parquet_path = candidate.resolve(strict=True)
        if not parquet_path.is_relative_to(dataset_root):
            raise ValueError(f"dataset table {name} escapes dataset root: {path_text}")
        if not parquet_path.is_file():
            raise ValueError(f"dataset table {name} is not a file: {path_text}")
        validated.append((name, parquet_path))

    for name, parquet_path in validated:
        try:
            ctx.register_parquet(name, str(parquet_path))
        except Exception as error:
            error.add_note(
                f"dataset table {name!r} canonical path: {parquet_path}"
            )
            raise
