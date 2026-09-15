import os
from pathlib import Path

import kat

import pyarrow as pa
from kat.dataprovider.trace_streamer import TraceStreamerProvider


NATIVE_HOOK_SUMMARY_SQL = """
SELECT
    event_type,
    COUNT(*) AS event_count,
    COALESCE(SUM(heap_size), 0) AS total_heap_size
FROM native_hook
WHERE event_type IS NOT NULL
GROUP BY event_type
ORDER BY event_type
"""

NATIVE_HOOK_SUMMARY_SCHEMA = pa.schema(
    [
        pa.field("event_type", pa.string(), nullable=False),
        pa.field("event_count", pa.int64(), nullable=False),
        pa.field("total_heap_size", pa.int64(), nullable=False),
    ]
)


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
    provider = TraceStreamerProvider(
        source=Path(source_path),
        executable=Path(executable),
        workspace_root=ctx.datasource_root,
    )
    return provider.query(NATIVE_HOOK_SUMMARY_SQL, schema=NATIVE_HOOK_SUMMARY_SCHEMA)
