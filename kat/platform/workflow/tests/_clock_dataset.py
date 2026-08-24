from __future__ import annotations

from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq


def write_clock_dataset(
    root: Path,
    *,
    pack: str = "example",
    source: str = "clocks",
    definitions: list[tuple[str, str, int]] | None = None,
    snapshots: list[tuple[int, str, int]] | None = None,
) -> dict[str, Path]:
    root = root / "sources" / pack / source / "tables"
    root.mkdir(parents=True, exist_ok=True)
    tables: dict[str, Path] = {}
    if definitions is not None:
        path = root / "clock_domain.parquet"
        pq.write_table(
            pa.Table.from_arrays(
                [
                    pa.array([row[0] for row in definitions], type=pa.string()),
                    pa.array([row[1] for row in definitions], type=pa.string()),
                    pa.array([row[2] for row in definitions], type=pa.uint64()),
                ],
                schema=pa.schema(
                    [
                        pa.field("clock_domain", pa.string(), nullable=False),
                        pa.field("clock_type", pa.string(), nullable=False),
                        pa.field("ticks_per_second", pa.uint64(), nullable=False),
                    ]
                ),
            ),
            path,
        )
        tables["clock_domain"] = path
    if snapshots is not None:
        path = root / "clock_snapshot.parquet"
        pq.write_table(
            pa.Table.from_arrays(
                [
                    pa.array([row[0] for row in snapshots], type=pa.uint64()),
                    pa.array([row[1] for row in snapshots], type=pa.string()),
                    pa.array([row[2] for row in snapshots], type=pa.uint64()),
                ],
                schema=pa.schema(
                    [
                        pa.field("snapshot_id", pa.uint64(), nullable=False),
                        pa.field("clock_domain", pa.string(), nullable=False),
                        pa.field("clock_value", pa.uint64(), nullable=False),
                    ]
                ),
            ),
            path,
        )
        tables["clock_snapshot"] = path
    return tables
