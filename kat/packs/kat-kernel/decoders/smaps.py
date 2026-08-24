"""把一份已采集的原始 SMAPS 文本机械解码为 Arrow facts。"""

from __future__ import annotations

from collections.abc import Iterable, Iterator
from pathlib import Path
import re

import pyarrow as pa


SNAPSHOTS_SCHEMA = pa.schema(
    [
        pa.field("snapshot_id", pa.uint64(), nullable=False),
        pa.field("source_file", pa.string(), nullable=False),
    ]
)

MAPPINGS_SCHEMA = pa.schema(
    [
        pa.field("snapshot_id", pa.uint64(), nullable=False),
        pa.field("start_address", pa.uint64(), nullable=False),
        pa.field("end_address", pa.uint64(), nullable=False),
        pa.field("permissions", pa.string(), nullable=False),
        pa.field("offset", pa.uint64(), nullable=False),
        pa.field("device", pa.string(), nullable=False),
        pa.field("inode", pa.uint64(), nullable=False),
        pa.field("pathname", pa.string(), nullable=False),
        pa.field("size_kib", pa.uint64(), nullable=False),
        pa.field("rss_kib", pa.uint64(), nullable=False),
        pa.field("pss_kib", pa.uint64(), nullable=False),
    ]
)

_HEADER = re.compile(
    r"(?P<start>[0-9A-Fa-f]+)-(?P<end>[0-9A-Fa-f]+)\s+"
    r"(?P<permissions>[r-][w-][x-][ps])\s+"
    r"(?P<offset>[0-9A-Fa-f]+)\s+"
    r"(?P<device>[0-9A-Fa-f]+:[0-9A-Fa-f]+)\s+"
    r"(?P<inode>[0-9]+)(?:\s+(?P<pathname>.*))?"
)
_PROPERTY = re.compile(r"[A-Za-z][A-Za-z0-9_()\-]*:\s*.*")
_KIB_METRIC = re.compile(
    r"(?P<name>Size|Rss|Pss):\s*(?P<value>[0-9]+)\s+kB"
)
_REQUIRED_METRICS = ("Size", "Rss", "Pss")
_UINT64_MAX = 2**64 - 1
_DEFAULT_BATCH_SIZE = 1024


class SmapsDecodeError(ValueError):
    """一份 SMAPS chunk 无法完整机械解码。"""


def decode_smaps_chunk(
    lines: Iterable[str], *, source: str = "<smaps chunk>"
) -> Iterator[dict[str, object]]:
    """逐条解码一份原始 SMAPS chunk，不负责打开文件或切分外层容器。"""

    current: dict[str, object] | None = None
    metrics: dict[str, int] = {}

    for line_number, raw_line in enumerate(lines, start=1):
        line = raw_line.rstrip("\r\n").strip()
        if not line:
            continue

        header = _HEADER.fullmatch(line)
        if header is not None:
            if current is not None:
                yield _complete_mapping(current, metrics, source)
            start_address = _u64(header.group("start"), 16, "start address", source)
            end_address = _u64(header.group("end"), 16, "end address", source)
            if end_address <= start_address:
                raise SmapsDecodeError(
                    f"{source}:{line_number}: mapping end address must be greater than its start address"
                )
            current = {
                "start_address": start_address,
                "end_address": end_address,
                "permissions": header.group("permissions"),
                "offset": _u64(header.group("offset"), 16, "offset", source),
                "device": header.group("device"),
                "inode": _u64(header.group("inode"), 10, "inode", source),
                "pathname": header.group("pathname") or "",
            }
            metrics = {}
            continue

        if current is None:
            raise SmapsDecodeError(
                f"{source}:{line_number}: expected a SMAPS mapping header"
            )
        if not _PROPERTY.fullmatch(line):
            raise SmapsDecodeError(
                f"{source}:{line_number}: invalid SMAPS metric or property"
            )

        name = line.partition(":")[0]
        if name not in _REQUIRED_METRICS:
            continue
        metric = _KIB_METRIC.fullmatch(line)
        if metric is None:
            raise SmapsDecodeError(
                f"{source}:{line_number}: {name} must be an unsigned KiB metric"
            )
        if name in metrics:
            raise SmapsDecodeError(
                f"{source}:{line_number}: duplicate {name} metric"
            )
        metrics[name] = _u64(metric.group("value"), 10, name, source)

    if current is not None:
        yield _complete_mapping(current, metrics, source)


def snapshots_reader(
    files: tuple[Path, ...], *, batch_size: int = _DEFAULT_BATCH_SIZE
) -> pa.RecordBatchReader:
    """为显式提供的文件序列建立稳定的 snapshot reader。"""

    paths = tuple(files)
    rows = (
        {"snapshot_id": snapshot_id, "source_file": str(path)}
        for snapshot_id, path in enumerate(paths)
    )
    return pa.RecordBatchReader.from_batches(
        SNAPSHOTS_SCHEMA,
        _record_batches(rows, SNAPSHOTS_SCHEMA, batch_size),
    )


def mappings_reader(
    files: tuple[Path, ...], *, batch_size: int = _DEFAULT_BATCH_SIZE
) -> pa.RecordBatchReader:
    """按文件输入顺序增量解码 mapping facts；重复路径不会去重。"""

    paths = tuple(files)

    def rows() -> Iterator[dict[str, object]]:
        for snapshot_id, path in enumerate(paths):
            with path.open("r", encoding="utf-8") as source_file:
                for mapping in decode_smaps_chunk(source_file, source=str(path)):
                    yield {"snapshot_id": snapshot_id, **mapping}

    return pa.RecordBatchReader.from_batches(
        MAPPINGS_SCHEMA,
        _record_batches(rows(), MAPPINGS_SCHEMA, batch_size),
    )


def _complete_mapping(
    mapping: dict[str, object], metrics: dict[str, int], source: str
) -> dict[str, object]:
    missing = [name for name in _REQUIRED_METRICS if name not in metrics]
    if missing:
        rendered = ", ".join(missing)
        raise SmapsDecodeError(f"{source}: mapping is missing required metrics: {rendered}")
    return {
        **mapping,
        "size_kib": metrics["Size"],
        "rss_kib": metrics["Rss"],
        "pss_kib": metrics["Pss"],
    }


def _record_batches(
    rows: Iterable[dict[str, object]], schema: pa.Schema, batch_size: int
) -> Iterator[pa.RecordBatch]:
    if type(batch_size) is not int or batch_size <= 0:
        raise ValueError("batch_size must be a positive integer")
    pending: list[dict[str, object]] = []
    for row in rows:
        pending.append(row)
        if len(pending) == batch_size:
            yield pa.RecordBatch.from_pylist(pending, schema=schema)
            pending = []
    if pending:
        yield pa.RecordBatch.from_pylist(pending, schema=schema)


def _u64(value: str, base: int, field: str, source: str) -> int:
    parsed = int(value, base)
    if parsed > _UINT64_MAX:
        raise SmapsDecodeError(f"{source}: {field} exceeds UInt64")
    return parsed
