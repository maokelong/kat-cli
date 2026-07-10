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
    if not states:
        state.nodes.append(_terminal_node(state, frame, "missing_state"))


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
    _process_frontier(facts, request, state)
    return _result(state)
