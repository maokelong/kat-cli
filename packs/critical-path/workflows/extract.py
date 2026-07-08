from __future__ import annotations

from kat import option, workflow

from lib.artifacts import artifact_queries
from lib.engine import analyze_critical_path
from lib.facts import query_window_facts


@workflow(title="Generic thread critical path")
@option("--root-itid", help="Root internal thread id. Preferred root thread selector.", default=0)
@option("--root-tid", help="Root OS thread id. Used when root-itid is not provided.", default=0)
@option("--root-pid", help="Root process id used with root-tid.", default=0)
@option("--root-thread-name-pattern", help="Root thread name regex fallback.", default="")
@option("--start-ts", help="Analysis window start timestamp in nanoseconds.", required=True)
@option("--end-ts", help="Analysis window end timestamp in nanoseconds.", required=True)
@option("--max-depth", help="Maximum wakeup dependency recursion depth.", default=8)
@option("--min-segment-ms", help="Minimum state segment duration in milliseconds.", default=0.1)
@option("--max-fact-rows", help="Maximum bounded rows fetched per fact query.", default=50000)
def extract(
    root_itid: int = 0,
    root_tid: int = 0,
    root_pid: int = 0,
    root_thread_name_pattern: str = "",
    start_ts: int = 0,
    end_ts: int = 0,
    max_depth: int = 8,
    min_segment_ms: float = 0.1,
    max_fact_rows: int = 50000,
):
    import kat

    if start_ts >= end_ts:
        raise ValueError("start_ts must be smaller than end_ts")
    if root_itid <= 0 and root_tid <= 0 and not root_thread_name_pattern:
        raise ValueError("one root thread selector is required")
    if max_depth < 0:
        raise ValueError("max_depth must be non-negative")
    if min_segment_ms <= 0:
        raise ValueError("min_segment_ms must be positive")
    if max_fact_rows <= 0:
        raise ValueError("max_fact_rows must be positive")

    resolved_root_itid, facts = query_window_facts(
        kat=kat,
        root_itid=root_itid,
        root_tid=root_tid,
        root_pid=root_pid,
        root_thread_name_pattern=root_thread_name_pattern,
        start_ts=start_ts,
        end_ts=end_ts,
        max_fact_rows=max_fact_rows,
    )
    result = analyze_critical_path(
        facts=facts,
        root_itid=resolved_root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        max_depth=max_depth,
        min_segment_ns=int(min_segment_ms * 1_000_000),
    )
    return artifact_queries(kat, result)
