import os
from pathlib import Path
from tempfile import TemporaryDirectory

import kat

from kat.pack.datasources import trace_streamer


@kat.workflow(
    name="summarize-native-hook",
    description="物化 Trace Streamer SQLite，并直接返回来源内聚合结果。",
    parameters={
        "source_path": "HiTrace file to decode.",
    },
    guide="workflows/summarize-native-hook.md",
)
def summarize_native_hook(
    ctx: kat.Context,
    source_path: str,
):
    """物化 Trace Streamer SQLite，并直接返回来源内聚合结果。"""
    executable = os.environ.get("KAT_TRACE_STREAMER_EXECUTABLE")
    if not executable:
        raise RuntimeError(
            "KAT_TRACE_STREAMER_EXECUTABLE must identify the approved parser"
        )
    with TemporaryDirectory(dir=ctx.scratch_root) as workspace:
        provider = trace_streamer.TraceStreamerProvider(
            source=Path(source_path),
            executable=Path(executable),
            workspace=Path(workspace) / "trace-streamer",
        ).decode()
        return provider.query(
            trace_streamer.NATIVE_HOOK_SUMMARY_SQL,
            schema=trace_streamer.NATIVE_HOOK_SUMMARY_SCHEMA,
        )
