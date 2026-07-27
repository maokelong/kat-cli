from __future__ import annotations

from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq
from datafusion import Expr, lit, udf

from .request import ResolvedDatasetRef


_DOMAIN_PATTERN = r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$"
_CLOCK_TYPES = {
    "boottime",
    "monotonic",
    "monotonic_coarse",
    "monotonic_raw",
    "realtime",
    "realtime_coarse",
    "ftrace_global",
    "ftrace_local",
}


class ClockCapability:
    """The KAT clock contract bound to one Workflow execution."""

    def __init__(self, dataset: ResolvedDatasetRef | None) -> None:
        resolver = ClockResolver(dataset)
        self._function = udf(
            resolver.convert_batch,
            [
                pa.string(),
                pa.uint64(),
                pa.string(),
            ],
            pa.uint64(),
            "stable",
            name="_kat_convert_clock_batch",
        )

    def convert(
        self,
        clock_domain: object,
        clock_value: object,
        *,
        target_domain: str,
    ) -> Expr:
        if type(target_domain) is not str or not target_domain:
            raise TypeError(
                "ctx.convert_clock target_domain must be an exact non-empty str"
            )
        if not isinstance(clock_domain, Expr) or not isinstance(clock_value, Expr):
            raise TypeError("ctx.convert_clock requires DataFusion Expr inputs")
        return self._function(
            clock_domain.cast(pa.string()),
            clock_value.cast(pa.uint64()),
            lit(target_domain),
        )


class ClockResolver:
    def __init__(self, dataset: ResolvedDatasetRef | None) -> None:
        self._dataset = dataset
        self._definitions: pa.Table | None = None
        self._baseline: pa.Table | None = None

    def convert_batch(
        self,
        clock_domains: object,
        clock_values: object,
        target_domains: object,
    ) -> pa.Array:
        domain_array = _arrow_array(clock_domains, pa.string())
        value_array = _arrow_array(clock_values, pa.uint64())
        target_array = _arrow_array(
            target_domains, pa.string(), length=len(domain_array)
        )
        if len(domain_array) != len(value_array) or len(domain_array) != len(target_array):
            raise ValueError("clock conversion inputs must have equal lengths")
        target_values = pc.unique(target_array)
        if len(target_values) != 1 or not target_values[0].is_valid:
            raise ValueError("clock conversion target_domain must be one non-empty literal")
        target = target_values[0]
        if not str(target):
            raise ValueError("clock conversion target_domain must be one non-empty literal")

        half_null = pc.xor(pc.is_null(domain_array), pc.is_null(value_array))
        if pc.any(half_null).equals(pa.scalar(True)):
            raise ValueError("clock_domain and clock_value must be null together")
        definitions = self._clock_definitions()
        if not _contains(definitions["clock_domain"], target):
            raise ValueError(
                f"unknown target clock domain {target}; "
                f"available domains: {_available_domains(definitions)}"
            )

        result = pa.nulls(len(domain_array), type=pa.uint64())
        for source in pc.unique(pc.drop_null(domain_array)):
            if not _contains(definitions["clock_domain"], source):
                raise ValueError(f"unknown source clock domain {source}")
            mask = pc.equal(domain_array, source)
            if source.equals(target):
                converted = value_array
            else:
                baseline = self._clock_baseline(definitions)
                source_base = _baseline_value(baseline, source)
                target_base = _baseline_value(baseline, target)
                converted = _translate_clock(value_array, mask, source_base, target_base)
            result = pc.if_else(mask, converted, result)
        return result

    def _clock_definitions(self) -> pa.Table:
        if self._definitions is not None:
            return self._definitions
        tables = self._tables()
        if "clock_domain" not in tables:
            raise ValueError("Dataset does not contain clock_domain evidence")
        definitions = pq.read_table(tables["clock_domain"])
        expected = pa.schema(
            [
                pa.field("clock_domain", pa.string(), nullable=False),
                pa.field("clock_type", pa.string(), nullable=False),
                pa.field("ticks_per_second", pa.uint64(), nullable=False),
            ]
        )
        if not definitions.schema.equals(expected, check_metadata=False):
            raise ValueError("clock_domain table has an invalid Schema")
        domains = definitions["clock_domain"]
        clock_types = definitions["clock_type"]
        ticks = definitions["ticks_per_second"]
        if (
            domains.null_count
            or clock_types.null_count
            or ticks.null_count
            or not _all_true(pc.match_substring_regex(domains, _DOMAIN_PATTERN))
            or len(pc.unique(domains)) != len(domains)
            or not _all_true(
                pc.is_in(clock_types, value_set=pa.array(sorted(_CLOCK_TYPES)))
            )
            or not _all_true(pc.equal(ticks, pa.scalar(1_000_000_000, pa.uint64())))
        ):
            raise ValueError("clock_domain definitions are invalid")
        self._definitions = definitions
        return definitions

    def _clock_baseline(self, definitions: pa.Table) -> pa.Table:
        if self._baseline is not None:
            return self._baseline
        tables = self._tables()
        if "clock_snapshot" not in tables:
            raise ValueError("clock conversion baseline is incomplete")
        snapshots = pq.read_table(tables["clock_snapshot"])
        expected = pa.schema(
            [
                pa.field("snapshot_id", pa.uint64(), nullable=False),
                pa.field("clock_domain", pa.string(), nullable=False),
                pa.field("clock_value", pa.uint64(), nullable=False),
            ]
        )
        if not snapshots.schema.equals(expected, check_metadata=False):
            raise ValueError("clock_snapshot table has an invalid Schema")
        if any(column.null_count for column in snapshots.columns):
            raise ValueError("clock_snapshot baseline has invalid domains")
        baseline = snapshots.filter(
            pc.equal(snapshots["snapshot_id"], pa.scalar(0, pa.uint64()))
        )
        domains = baseline["clock_domain"]
        if (
            len(pc.unique(domains)) != len(domains)
            or not _all_true(
                pc.is_in(domains, value_set=definitions["clock_domain"])
            )
        ):
            raise ValueError("clock_snapshot baseline has invalid domains")
        self._baseline = baseline
        return baseline

    def _tables(self) -> dict[str, Path]:
        if self._dataset is None:
            raise ValueError("clock conversion requires a Dataset")
        return self._dataset.tables


def _all_true(value: pa.Array | pa.ChunkedArray) -> bool:
    result = pc.all(value)
    return result.is_valid and result.equals(pa.scalar(True))


def _contains(values: pa.Array | pa.ChunkedArray, value: pa.Scalar) -> bool:
    return pc.any(pc.equal(values, value)).equals(pa.scalar(True))


def _available_domains(definitions: pa.Table) -> str:
    ordered = pc.take(
        definitions["clock_domain"], pc.sort_indices(definitions["clock_domain"])
    )
    return ", ".join(str(domain) for domain in ordered)


def _baseline_value(baseline: pa.Table, domain: pa.Scalar) -> pa.Scalar:
    values = pc.filter(
        baseline["clock_value"], pc.equal(baseline["clock_domain"], domain)
    )
    if len(values) != 1:
        raise ValueError("clock conversion baseline is incomplete")
    return values[0]


def _arrow_array(
    value: object, data_type: pa.DataType, length: int | None = None
) -> pa.Array:
    if isinstance(value, pa.Array):
        if value.type != data_type:
            raise TypeError(f"clock conversion requires {data_type}, got {value.type}")
        return value
    if isinstance(value, pa.Scalar):
        if value.type != data_type or length is None:
            raise TypeError(f"clock conversion requires {data_type}")
        return pa.repeat(value, length)
    raise TypeError("clock conversion requires Arrow arrays or scalars")


def _translate_clock(
    values: pa.Array,
    mask: pa.Array,
    source_base: pa.Scalar,
    target_base: pa.Scalar,
) -> pa.Array:
    safe = pc.if_else(mask, values, source_base)
    goes_up = pc.greater_equal(safe, source_base)
    up_values = pc.if_else(goes_up, safe, source_base)
    down_values = pc.if_else(goes_up, source_base, safe)
    upward = pc.add_checked(target_base, pc.subtract_checked(up_values, source_base))
    downward = pc.subtract_checked(
        target_base, pc.subtract_checked(source_base, down_values)
    )
    return pc.if_else(goes_up, upward, downward)
