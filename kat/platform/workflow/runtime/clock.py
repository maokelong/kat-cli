from __future__ import annotations

import pyarrow as pa
import pyarrow.compute as pc
from datafusion import Expr, SessionContext, lit, udf

from kat._identifiers import valid_source_name


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

    def __init__(self, session: SessionContext, current_pack: str) -> None:
        self._session = session
        self._current_pack = current_pack
        self._resolvers: dict[tuple[str, str], ClockResolver] = {}

    def convert(
        self,
        clock_domain: object,
        clock_value: object,
        *,
        source: str,
        target_domain: str,
        pack: str | None = None,
    ) -> Expr:
        if type(source) is not str or not valid_source_name(source):
            raise TypeError("ctx.convert_clock source must be an exact valid Source name")
        if type(target_domain) is not str or not target_domain:
            raise TypeError(
                "ctx.convert_clock target_domain must be an exact non-empty str"
            )
        if pack is not None and (type(pack) is not str or not pack):
            raise TypeError("ctx.convert_clock pack must be an exact non-empty str or None")
        if not isinstance(clock_domain, Expr) or not isinstance(clock_value, Expr):
            raise TypeError("ctx.convert_clock requires DataFusion Expr inputs")
        identity = (self._current_pack if pack is None else pack, source)
        resolver = self._resolvers.get(identity)
        if resolver is None:
            resolver = ClockResolver(self._session, *identity)
            resolver.prepare()
            self._resolvers[identity] = resolver
        return resolver.function(
            clock_domain.cast(pa.string()),
            clock_value.cast(pa.uint64()),
            lit(target_domain),
        )


class ClockResolver:
    def __init__(self, session: SessionContext, pack: str, source: str) -> None:
        self._session = session
        self._pack = pack
        self._source = source
        self._definitions: pa.Table | None = None
        self._baseline: pa.Table | None = None
        self._baseline_error: Exception | None = None
        self.function = udf(
            self.convert_batch,
            [
                pa.string(),
                pa.uint64(),
                pa.string(),
            ],
            pa.uint64(),
            "stable",
            name=f"_kat_convert_clock_{source}",
        )

    def prepare(self) -> None:
        self._clock_definitions()
        try:
            self._clock_baseline(self._definitions)
        except Exception as error:
            # A same-domain conversion does not consume the baseline. Preserve
            # the original contract by surfacing this error only if translation
            # between two domains is actually requested by a batch.
            self._baseline_error = error

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
                if self._baseline_error is not None:
                    raise self._baseline_error
                baseline = self._clock_baseline(definitions)
                source_base = _baseline_value(baseline, source)
                target_base = _baseline_value(baseline, target)
                converted = _translate_clock(value_array, mask, source_base, target_base)
            result = pc.if_else(mask, converted, result)
        return result

    def _clock_definitions(self) -> pa.Table:
        if self._definitions is not None:
            return self._definitions
        definitions = self._read_table("clock_domain")
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
        try:
            snapshots = self._read_table("clock_snapshot")
        except Exception as error:
            raise ValueError("clock conversion baseline is incomplete") from error
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

    def _read_table(self, table: str) -> pa.Table:
        pack = self._pack.replace('"', '""')
        source = self._source.replace('"', '""')
        table_name = table.replace('"', '""')
        frame = self._session.sql(
            f'SELECT * FROM "{pack}"."{source}"."{table_name}"'
        )
        return pa.Table.from_batches(frame.collect(), schema=frame.schema())


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
