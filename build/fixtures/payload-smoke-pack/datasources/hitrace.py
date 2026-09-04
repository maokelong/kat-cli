from __future__ import annotations

import os
import time
import unicodedata
from pathlib import Path
from typing import Self

import pyarrow.parquet as pq
from kat import dataprovider as dp
from kat_datasource import hitrace


_EXPECTED_RELATIONS = ("clock_domain", "clock_snapshot")
_EXPECTED_COLUMNS = {
    "clock_domain": (
        ("clock_domain", "string", False),
        ("clock_type", "string", False),
        ("ticks_per_second", "uint64", False),
    ),
    "clock_snapshot": (
        ("snapshot_id", "uint64", False),
        ("clock_domain", "string", False),
        ("clock_value", "uint64", False),
    ),
}
_WINDOWS_DEVICE_NAMES = frozenset(
    {"con", "prn", "aux", "nul"}
    | {f"com{index}" for index in range(1, 10)}
    | {f"lpt{index}" for index in range(1, 10)}
)
_WINDOWS_FORBIDDEN_CHARACTERS = frozenset('<>:"/\\|?*')
# 仅 Payload CI 设置，让两个 Runtime 在确认目标缺失后同步启动 decode。
_PAYLOAD_SMOKE_BARRIER_ENVIRONMENT_VARIABLE = "KAT_PAYLOAD_SMOKE_BARRIER"


class HitraceProvider:
    """Minimal PACK-owned Provider used to exercise the shipped decode API."""

    def __init__(self, *, source: Path, datasource_root: Path) -> None:
        for field, value in (("source", source), ("datasource_root", datasource_root)):
            if not isinstance(value, Path):
                raise TypeError(f"Payload smoke {field} must be a pathlib.Path")
        if not datasource_root.is_dir():
            raise RuntimeError("Payload smoke datasource_root must be a directory")
        self._source = source
        self._destination = datasource_root.resolve(strict=True) / _source_stem(source)
        self._query_provider: dp.DataFusionProvider | None = None

    def prepare(self) -> Self:
        self._query_provider = None
        if _path_exists(self._destination):
            self._open_and_validate()
            return self
        if not self._source.is_file():
            raise RuntimeError("Payload smoke source must be an existing file")
        self._source = self._source.resolve(strict=True)
        barrier = _wait_for_payload_smoke_barrier(self._destination)
        try:
            hitrace.decode(self._source, self._destination)
        except hitrace.DecodeError:
            if _path_exists(self._destination):
                self._open_and_validate()
                _record_payload_smoke_outcome(barrier, "reused")
                return self
            raise
        self._open_and_validate()
        _record_payload_smoke_outcome(barrier, "published")
        return self

    def _open_and_validate(self) -> None:
        catalog = dp.open(root=self._destination)
        if catalog.tables != _EXPECTED_RELATIONS:
            raise RuntimeError(
                "Payload smoke materialization has incompatible relations"
            )
        query_provider = dp.DataFusionProvider(catalog=catalog)
        for relation, expected in _EXPECTED_COLUMNS.items():
            schema = pq.read_schema(self._destination / f"{relation}.parquet")
            metadata = schema.metadata or {}
            expected_version = hitrace.MATERIALIZATION_VERSION.encode("utf-8")
            if (
                metadata.get(hitrace.MATERIALIZATION_VERSION_METADATA_KEY)
                != expected_version
            ):
                raise RuntimeError(
                    f"Payload smoke materialization relation {relation!r} "
                    "has an incompatible version"
                )
            actual = tuple(
                (field.name, str(field.type), field.nullable) for field in schema
            )
            if actual != expected:
                raise RuntimeError(
                    f"Payload smoke materialization relation {relation!r} "
                    "has incompatible columns"
                )
        self._query_provider = query_provider

    def query(self, sql: str) -> dp.Table:
        if self._query_provider is None:
            raise RuntimeError("prepare must be called before query")
        return self._query_provider.query(sql)


def _source_stem(source: Path) -> str:
    stem = source.stem
    device_name = stem.split(".", 1)[0].casefold()
    if (
        not stem
        or stem in {".", ".."}
        or stem.endswith((".", " "))
        or device_name in _WINDOWS_DEVICE_NAMES
        or any(
            character in _WINDOWS_FORBIDDEN_CHARACTERS
            or unicodedata.category(character) == "Cc"
            for character in stem
        )
    ):
        raise ValueError(f"invalid Payload smoke source stem: {stem!r}")
    return stem


def _path_exists(path: Path) -> bool:
    return path.exists() or path.is_symlink()


def _wait_for_payload_smoke_barrier(destination: Path) -> Path | None:
    selected = os.environ.get(_PAYLOAD_SMOKE_BARRIER_ENVIRONMENT_VARIABLE)
    if selected is None:
        return None
    barrier = Path(selected)
    try:
        resolved = barrier.resolve(strict=True)
    except (OSError, RuntimeError):
        raise RuntimeError("Payload smoke barrier path is invalid") from None
    junction = getattr(barrier, "is_junction", None)
    if (
        resolved != barrier
        or not resolved.is_dir()
        or barrier.is_symlink()
        or (junction is not None and junction())
    ):
        raise RuntimeError("Payload smoke barrier must be an ordinary directory")

    process_id = os.getpid()
    pending = resolved / f".ready-{process_id}.tmp"
    ready = resolved / f"ready-{process_id}"
    pending.write_text(str(destination.resolve(strict=False)), encoding="utf-8")
    pending.replace(ready)
    release = resolved / "release"
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        if release.is_file() and not release.is_symlink():
            return resolved
        if _path_exists(release):
            raise RuntimeError("Payload smoke barrier release is invalid")
        time.sleep(0.02)
    raise RuntimeError("Payload smoke barrier timed out")


def _record_payload_smoke_outcome(barrier: Path | None, outcome: str) -> None:
    if barrier is None:
        return
    process_id = os.getpid()
    pending = barrier / f".outcome-{process_id}.tmp"
    outcome_marker = barrier / f"outcome-{process_id}"
    pending.write_text(outcome, encoding="utf-8")
    pending.replace(outcome_marker)
