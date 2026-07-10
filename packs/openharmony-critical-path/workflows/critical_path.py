from functools import partial

from kat import workflow

from compute.critical_path import extract_critical_path
from compute.models import CriticalPathRequest, FactProvider
from facts.callstacks import callstack_slices
from facts.scheduling import sched_slices, wakeup_edges
from facts.threads import thread_metadata, thread_state_segments


def fact_provider(kat) -> FactProvider:
    return FactProvider(
        thread_metadata=partial(thread_metadata, kat),
        thread_state_segments=partial(thread_state_segments, kat),
        wakeup_edges=partial(wakeup_edges, kat),
        sched_slices=partial(sched_slices, kat),
        callstack_slices=partial(callstack_slices, kat),
    )


@workflow(title="Critical path", description="Extract a critical path from a root thread and time window")
def critical_path(kat, root_itid: int, start_ts: int, end_ts: int,
                  max_depth: int = 8, min_segment_ms: float = 0.1):
    result = extract_critical_path(
        fact_provider(kat),
        CriticalPathRequest(root_itid, start_ts, end_ts, max_depth, min_segment_ms),
    )
    return {"path_nodes": result.nodes, "path_edges": result.edges}
