from pathlib import Path

import kat

from kat.pack.datasources.ftrace import FtraceTextProvider


@kat.workflow(
    name="summarize-ftrace-events",
    title="Summarize Ftrace events",
    required_tables=[],
    parameters={
        "trace_path": "Path to a tracefs text file.",
        "clock_domain": "Clock domain assigned by the capture configuration.",
    },
)
def summarize_ftrace_events(
    ctx: kat.Context,
    trace_path: str,
    clock_domain: str,
):
    """Parse one Ftrace text file and publish event counts."""
    provider = FtraceTextProvider(
        source=Path(trace_path),
        materialization_root=ctx.datasource_root / "ftrace-text",
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
