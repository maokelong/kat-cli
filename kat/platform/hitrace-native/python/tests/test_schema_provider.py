from pathlib import Path

from datafusion import SessionContext

from _kat_hitrace import HitraceSchemaProvider


HEADER_SIZE = 1024
HEADER_MAGIC = 0x464F_5250_534F_484F


def _write_header_only_trace(path: Path) -> None:
    data = bytearray(HEADER_SIZE)
    data[0:8] = HEADER_MAGIC.to_bytes(8, "little")
    data[8:16] = HEADER_SIZE.to_bytes(8, "little")
    for offset, value in zip((60, 68, 76, 84, 92, 100), range(1, 7), strict=True):
        data[offset : offset + 8] = value.to_bytes(8, "little")
    path.write_bytes(data)


def _count(ctx: SessionContext, schema: str, table: str) -> int:
    batches = ctx.sql(f'SELECT COUNT(*) AS count FROM "{schema}"."{table}"').collect()
    return batches[0].column(0)[0].as_py()


def test_official_schema_provider_capsule_is_queryable_after_source_disappears(
    tmp_path: Path,
) -> None:
    trace = tmp_path / "capture.htrace"
    _write_header_only_trace(trace)
    provider = HitraceSchemaProvider(trace)
    trace.unlink()

    ctx = SessionContext()
    ctx.catalog().register_schema("automatic", provider)
    capsule = provider.__datafusion_schema_provider__(ctx)
    ctx.catalog().register_schema("capsule", capsule)

    assert set(ctx.catalog().schema("automatic").names()) == {
        "clock_domain",
        "clock_snapshot",
    }
    assert _count(ctx, "automatic", "clock_domain") == 6
    assert _count(ctx, "automatic", "clock_snapshot") == 6
    assert _count(ctx, "capsule", "clock_domain") == 6
