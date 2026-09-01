from pathlib import Path

from kat.pack.datasources.ftrace import FtraceProvider

import kat

SUMMARY_SQL = """
SELECT
    h.tracer,
    h.cpu_count,
    h.entries_in_buffer AS source_event_count,
    COUNT(e._kat_row_id) AS supported_event_count
FROM text_ftrace_header h
CROSS JOIN text_ftrace_event e
GROUP BY h.tracer, h.cpu_count, h.entries_in_buffer
"""


@kat.workflow(
    name="summarize-ftrace",
    title="Summarize typed Ftrace events",
    required_tables=[],
    parameters={
        "trace_path": "Path to an uncompressed UTF-8 text Ftrace file.",
        "clock_domain": "Clock domain assigned by the capture configuration.",
    },
)
def summarize_ftrace(
    ctx: kat.Context,
    trace_path: str,
    clock_domain: str,
):
    """转换文本 Ftrace，并发布来源事件与已支持事件数量。"""
    provider = FtraceProvider(
        source=Path(trace_path),
        clock_domain=clock_domain,
        workspace_root=ctx.datasource_root,
    )
    return provider.query(SUMMARY_SQL)
