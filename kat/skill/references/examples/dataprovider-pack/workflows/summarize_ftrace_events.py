from pathlib import Path
from tempfile import TemporaryDirectory

import kat

from kat.pack.datasources.ftrace import FtraceTextProvider


@kat.workflow(
    name="summarize-ftrace-events",
    description="解析一份 Ftrace 文本并发布事件计数。",
    parameters={
        "trace_path": "Path to a tracefs text file.",
        "clock_domain": "Clock domain assigned by the capture configuration.",
    },
    guide="workflows/summarize-ftrace-events.md",
)
def summarize_ftrace_events(
    ctx: kat.Context,
    trace_path: str,
    clock_domain: str,
):
    """解析一份 Ftrace 文本并发布事件计数。"""
    with TemporaryDirectory(dir=ctx.datasource_root) as workspace:
        provider = FtraceTextProvider(
            source=Path(trace_path),
            catalog_root=Path(workspace) / "catalog",
            clock_domain=clock_domain,
        ).decode()
        return provider.query(
            """
            SELECT event, COUNT(*) AS event_count
            FROM events
            GROUP BY event
            ORDER BY event_count DESC, event
            """
        )
