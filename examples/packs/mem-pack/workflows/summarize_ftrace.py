from pathlib import Path

from kat.pack.datasources.ftrace import FtraceProvider
from kat.pack.helpers.html_chart import HtmlReport, PieChartUnit

import kat

SUMMARY_SQL = """
SELECT
    h.tracer,
    COUNT(e._kat_row_id) AS supported_event_count,
    COUNT(DISTINCT e.cpu) AS observed_cpu_count
FROM text_ftrace_header h
CROSS JOIN text_ftrace_event e
GROUP BY h.tracer
"""

EMPTY_SUMMARY_SQL = """
SELECT
    tracer,
    CAST(0 AS BIGINT) AS supported_event_count,
    CAST(0 AS BIGINT) AS observed_cpu_count
FROM text_ftrace_header
"""


@kat.workflow(
    name="summarize-ftrace",
    description="Summarize typed Ftrace events.",
    parameters={
        "trace_path": "Path to an uncompressed UTF-8 text Ftrace file.",
        "clock_domain": "Clock domain assigned by the capture configuration.",
        "html_path": "Optional destination for an ECharts summary page.",
    },
)
def summarize_ftrace(
    ctx: kat.Context,
    trace_path: str,
    clock_domain: str,
    html_path: str | None = None,
):
    """转换文本 Ftrace，并汇总已支持事件与实际出现的 CPU。"""
    provider = FtraceProvider(
        source=Path(trace_path),
        clock_domain=clock_domain,
        workspace_root=ctx.datasource_root,
    )
    sql = SUMMARY_SQL if "text_ftrace_event" in provider.tables else EMPTY_SUMMARY_SQL
    summary = provider.query(sql)
    if html_path is not None:
        report = HtmlReport("Ftrace summary")
        report.add_tab("Overview").add(
            PieChartUnit(
                summary,
                category="tracer",
                value="supported_event_count",
                title="Supported Ftrace events by tracer",
            )
        )
        report.write(Path(html_path))
    return summary
