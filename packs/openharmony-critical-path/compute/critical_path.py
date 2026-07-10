from __future__ import annotations

from dataclasses import asdict
from typing import Any

import pyarrow as pa
from datafusion import SessionContext
from kat import compute

from .models import (
    PATH_EDGE_SCHEMA,
    PATH_NODE_SCHEMA,
    CriticalPathRequest,
    CriticalPathResult,
    FactProvider,
    PathEdge,
    PathNode,
    TraversalFrame,
    TraversalState,
)


def _dataframe(rows: list[dict[str, Any]], schema: pa.Schema):
    ctx = SessionContext()
    return ctx.from_arrow(pa.Table.from_pylist(rows, schema=schema))


def _validate_request(request: CriticalPathRequest) -> None:
    if request.start_ts >= request.end_ts:
        raise ValueError("start_ts must be less than end_ts")
    if request.max_depth < 0:
        raise ValueError("max_depth must be non-negative")
    if request.min_segment_ms < 0:
        raise ValueError("min_segment_ms must be non-negative")


def _rows(dataframe) -> list[dict[str, Any]]:
    return [row for batch in dataframe.collect() for row in batch.to_pylist()]


def _overlap(row: dict[str, Any], start_ts: int, end_ts: int) -> int:
    return max(0, min(row["ts"] + row["dur"], end_ts) - max(row["ts"], start_ts))


def _overlapping_sched(facts: FactProvider, itid: int, start_ts: int, end_ts: int) -> list[dict[str, Any]]:
    return [row for row in _rows(facts.sched_slices(itid, start_ts, end_ts))
            if _overlap(row, start_ts, end_ts) > 0]


def _overlapping_callstacks(
    facts: FactProvider, itid: int, start_ts: int, end_ts: int
) -> list[dict[str, Any]]:
    return [row for row in _rows(facts.callstack_slices(itid, start_ts, end_ts))
            if _overlap(row, start_ts, end_ts) > 0]


def _best_evidence(rows: list[dict[str, Any]], start_ts: int, end_ts: int) -> dict[str, Any] | None:
    if not rows:
        return None
    return min(rows, key=lambda row: (-_overlap(row, start_ts, end_ts), row["ts"], row.get("name") or ""))


def _blocked_classification(state_name: str, row: dict[str, Any]) -> str:
    if state_name == "D-IO" or row.get("iowait") == 1:
        return "io_block"
    if state_name in {"D", "D-NIO"} and row.get("blocked_caller"):
        return "non_io_block"
    return "unknown"


def _terminal_node(state: TraversalState, frame: TraversalFrame, reason: str) -> PathNode:
    metadata = state.metadata.get(frame.itid, {})
    node = PathNode(
        node_id=state.next_node_id,
        depth=frame.depth,
        itid=frame.itid,
        tid=metadata.get("tid"),
        thread_name=metadata.get("thread_name"),
        pid=metadata.get("pid"),
        process_name=metadata.get("process_name"),
        window_start_ts=frame.start_ts,
        window_end_ts=frame.end_ts,
        classification=reason,
        uncertainty=reason,
        termination_reason=reason,
    )
    state.next_node_id += 1
    return node


def _process_frontier(facts: FactProvider, request: CriticalPathRequest, state: TraversalState) -> None:
    frame = state.frontier.pop()
    metadata = _rows(facts.thread_metadata(frame.itid))
    if metadata:
        state.metadata[frame.itid] = metadata[0]
    states = _rows(facts.thread_state_segments(frame.itid, frame.start_ts, frame.end_ts))
    clipped = [
        (row, max(row["ts"], frame.start_ts), min(row["ts"] + row["dur"], frame.end_ts))
        for row in states
        if max(row["ts"], frame.start_ts) < min(row["ts"] + row["dur"], frame.end_ts)
    ]
    if not clipped:
        state.nodes.append(_terminal_node(state, frame, "missing_state"))
        return

    row, segment_start, segment_end = max(
        clipped, key=lambda item: (item[2], item[1], item[0]["state"])
    )
    state_name = row["state"]
    sched = None
    callstack = None
    if state_name == "Running":
        sched = _best_evidence(
            _overlapping_sched(facts, frame.itid, segment_start, segment_end),
            segment_start,
            segment_end,
        )
        callstack = _best_evidence(
            _overlapping_callstacks(facts, frame.itid, segment_start, segment_end),
            segment_start,
            segment_end,
        )
        classification = "self_running" if sched else "unknown"
        uncertainty = None if sched else "missing_sched_evidence"
    elif state_name in {"R", "R+"}:
        classification = "scheduler_wait"
        uncertainty = None
    else:
        classification = _blocked_classification(state_name, row)
        uncertainty = None if classification != "unknown" else "unsupported_state"

    metadata_row = state.metadata.get(frame.itid, {})
    node = PathNode(
        node_id=state.next_node_id,
        depth=frame.depth,
        itid=frame.itid,
        tid=metadata_row.get("tid"),
        thread_name=metadata_row.get("thread_name"),
        pid=metadata_row.get("pid"),
        process_name=metadata_row.get("process_name"),
        window_start_ts=frame.start_ts,
        window_end_ts=frame.end_ts,
        segment_start_ts=segment_start,
        segment_end_ts=segment_end,
        dur=segment_end - segment_start,
        state=state_name,
        classification=classification,
        sched_cpu=sched.get("cpu") if sched else None,
        sched_priority=sched.get("priority") if sched else None,
        callstack_name=callstack.get("name") if callstack else None,
        blocked_caller=row.get("blocked_caller"),
        confidence="fact",
        uncertainty=uncertainty,
    )
    state.next_node_id += 1
    state.nodes.append(node)

    if frame.next_node_id is not None:
        state.edges.append(PathEdge(
            edge_id=state.next_edge_id,
            from_node_id=node.node_id,
            to_node_id=frame.next_node_id,
            from_itid=frame.itid,
            to_itid=frame.itid,
            parent_depth=frame.depth,
            child_depth=frame.depth,
            edge_type="sequence",
            confidence="fact",
            reason="thread_state_order",
        ))
        state.next_edge_id += 1

    if segment_start > frame.start_ts:
        state.frontier.append(TraversalFrame(
            itid=frame.itid,
            start_ts=frame.start_ts,
            end_ts=segment_start,
            depth=frame.depth,
            next_node_id=node.node_id,
        ))


def _result(state: TraversalState) -> CriticalPathResult:
    return CriticalPathResult(
        nodes=_dataframe([asdict(node) for node in state.nodes], PATH_NODE_SCHEMA),
        edges=_dataframe([asdict(edge) for edge in state.edges], PATH_EDGE_SCHEMA),
    )


def target_not_found_result() -> CriticalPathResult:
    state = TraversalState(nodes=[
        PathNode(
            node_id=1,
            depth=0,
            classification="target_not_found",
            uncertainty="target_not_found",
            termination_reason="target_not_found",
        )
    ])
    return _result(state)


@compute(title="Critical path", description="Traverse normalized trace facts into a critical path")
def extract_critical_path(facts: FactProvider, request: CriticalPathRequest) -> CriticalPathResult:
    _validate_request(request)
    state = TraversalState(frontier=[
        TraversalFrame(request.root_itid, request.start_ts, request.end_ts, depth=0)
    ])
    while state.frontier:
        _process_frontier(facts, request, state)
    return _result(state)
