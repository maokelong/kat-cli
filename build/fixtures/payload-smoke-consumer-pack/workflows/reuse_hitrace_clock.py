from __future__ import annotations

import unicodedata
from pathlib import Path

import kat
import pyarrow.parquet as pq
from kat import dataprovider as dp


_EXPECTED_RELATIONS = ("clock_domain", "clock_snapshot")
_MATERIALIZATION_VERSION_METADATA_KEY = b"kat.materialization.version"
_MATERIALIZATION_VERSION = b"hitrace-v1"
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


@kat.workflow(
    name="reuse-hitrace-clock",
    description="Read a compatible Hitrace materialization published by another PACK.",
    parameters={"trace_path": "Lexical path used to derive the shared Source stem."},
)
def reuse_hitrace_clock(ctx: kat.Context, trace_path: str):
    """Validate and query a shared materialization without decoding the Source."""
    destination = ctx.datasource_root / _source_stem(Path(trace_path))
    catalog = dp.open(root=destination)
    if catalog.tables != _EXPECTED_RELATIONS:
        raise RuntimeError(
            "Payload smoke consumer materialization has incompatible relations"
        )
    query_provider = dp.DataFusionProvider(catalog=catalog)
    for relation, expected in _EXPECTED_COLUMNS.items():
        schema = pq.read_schema(destination / f"{relation}.parquet")
        metadata = schema.metadata or {}
        if (
            metadata.get(_MATERIALIZATION_VERSION_METADATA_KEY)
            != _MATERIALIZATION_VERSION
        ):
            raise RuntimeError(
                f"Payload smoke consumer relation {relation!r} "
                "has an incompatible version"
            )
        actual = tuple(
            (field.name, str(field.type), field.nullable) for field in schema
        )
        if actual != expected:
            raise RuntimeError(
                f"Payload smoke consumer relation {relation!r} "
                "has incompatible columns"
            )
    return query_provider.query(
        """
        SELECT clock_domain, clock_value
        FROM clock_snapshot
        WHERE snapshot_id = 0 AND clock_domain = 'boottime'
        """
    )


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
        raise ValueError(f"invalid Payload smoke consumer source stem: {stem!r}")
    return stem
