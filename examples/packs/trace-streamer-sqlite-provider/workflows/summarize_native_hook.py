from pathlib import Path

import kat

from kat.pack.datasources import trace_streamer


@kat.workflow(
    name="summarize-native-hook",
    title="Summarize native hook events",
    required_tables=[],
    parameters={
        "source_path": "HiTrace file to decode.",
        "trace_streamer_path": "Trace Streamer executable.",
    },
)
def summarize_native_hook(
    ctx: kat.Context,
    source_path: str,
    trace_streamer_path: str,
):
    """物化 Trace Streamer SQLite，并直接返回来源内聚合结果。"""
    provider = trace_streamer.TraceStreamerProvider(
        source=Path(source_path),
        executable=Path(trace_streamer_path),
        materialization_root=ctx.datasource_root / "trace-streamer",
    )
    return provider.decode().query(
        trace_streamer.NATIVE_HOOK_SUMMARY_SQL,
        schema=trace_streamer.NATIVE_HOOK_SUMMARY_SCHEMA,
    )
