from __future__ import annotations

from dataclasses import asdict, replace
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


class FactContractError(RuntimeError):
    """A normalized fact does not satisfy the compute contract."""


_INTEGER = "integer"
_STRING = "string"
IO_THREAD_NAMES = frozenset({
    "fsverity", "cdecrypt", "erofs_unzipd", "fsignature", "hmfs",
    "wk:0/0/0", "wk:2/1/0", "wk:0/-20/0",
})
IO_THREAD_EXCLUSIONS = frozenset({"hmfs_txn"})
_FACT_COLUMNS = {
    "thread_metadata": {
        "itid": _INTEGER, "tid": _INTEGER, "thread_name": _STRING,
        "pid": _INTEGER, "process_name": _STRING,
    },
    "thread_state_segments": {
        "itid": _INTEGER, "ts": _INTEGER, "dur": _INTEGER, "state": _STRING,
        "cpu": _INTEGER, "arg_setid": _INTEGER, "iowait": _INTEGER,
        "blocked_caller": _STRING,
    },
    "sched_slices": {
        "itid": _INTEGER, "ts": _INTEGER, "dur": _INTEGER, "ts_end": _INTEGER,
        "cpu": _INTEGER, "priority": _INTEGER, "end_state": _STRING,
    },
    "callstack_slices": {
        "itid": _INTEGER, "ts": _INTEGER, "dur": _INTEGER, "name": _STRING,
    },
    "wakeup_edges": {
        "wakeup_ts": _INTEGER, "target_itid": _INTEGER,
        "waker_itid": _INTEGER, "name": _STRING,
    },
}


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


def _fact_error(fact_name: str, itid: int, start_ts: int, end_ts: int, detail: str) -> FactContractError:
    return FactContractError(
        f"fact contract error: capability={fact_name}, itid={itid}, "
        f"start_ts={start_ts}, end_ts={end_ts}: {detail}"
    )


def _compatible_type(actual: pa.DataType, expected: str) -> bool:
    if expected == _INTEGER:
        return pa.types.is_integer(actual)
    return pa.types.is_string(actual) or pa.types.is_large_string(actual)


def _fact_rows(
    callback, fact_name: str, itid: int, start_ts: int, end_ts: int, *args
) -> list[dict[str, Any]]:
    try:
        dataframe = callback(*args)
        schema = dataframe.schema()
        batches = dataframe.collect()
    except Exception as error:
        raise _fact_error(fact_name, itid, start_ts, end_ts, "collection failed") from error

    required = _FACT_COLUMNS[fact_name]
    available = set(schema.names)
    missing = sorted(set(required) - available)
    if missing:
        raise _fact_error(
            fact_name, itid, start_ts, end_ts,
            f"missing required columns: {', '.join(missing)}",
        )
    for name, expected in required.items():
        actual = schema.field(name).type
        if not _compatible_type(actual, expected):
            raise _fact_error(
                fact_name, itid, start_ts, end_ts,
                f"incompatible column type: {name} expected {expected}, got {actual}",
            )
    try:
        return [row for batch in batches for row in batch.to_pylist()]
    except Exception as error:
        raise _fact_error(fact_name, itid, start_ts, end_ts, "conversion failed") from error


def _overlap(row: dict[str, Any], start_ts: int, end_ts: int) -> int:
    return max(0, min(row["ts"] + row["dur"], end_ts) - max(row["ts"], start_ts))


def _overlapping_sched(facts: FactProvider, itid: int, start_ts: int, end_ts: int) -> list[dict[str, Any]]:
    return [row for row in _fact_rows(
        facts.sched_slices, "sched_slices", itid, start_ts, end_ts, itid, start_ts, end_ts
    )
            if _overlap(row, start_ts, end_ts) > 0]


def _overlapping_callstacks(
    facts: FactProvider, itid: int, start_ts: int, end_ts: int
) -> list[dict[str, Any]]:
    return [row for row in _fact_rows(
        facts.callstack_slices, "callstack_slices", itid, start_ts, end_ts, itid, start_ts, end_ts
    )
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


def _append_node(
    state: TraversalState,
    frame: TraversalFrame,
    *,
    itid: int | None = None,
    thread_name: str | None = None,
    classification: str | None = None,
    uncertainty: str | None = None,
    termination_reason: str | None = None,
) -> PathNode:
    metadata = state.metadata.get(frame.itid, {})
    node = PathNode(
        node_id=state.next_node_id,
        depth=frame.depth,
        itid=frame.itid if itid is None else itid,
        tid=metadata.get("tid"),
        thread_name=thread_name or metadata.get("thread_name"),
        pid=metadata.get("pid"),
        process_name=metadata.get("process_name"),
        window_start_ts=frame.start_ts,
        window_end_ts=frame.end_ts,
        classification=classification,
        blocking_context_node_id=frame.blocking_context_node_id,
        inherited_blocked_caller=frame.inherited_blocked_caller,
        uncertainty=uncertainty,
        termination_reason=termination_reason,
    )
    state.next_node_id += 1
    state.nodes.append(node)
    return node


def _terminal_node(
    state: TraversalState,
    frame: TraversalFrame,
    reason: str,
    *,
    itid: int | None = None,
    thread_name: str | None = None,
    uncertainty: str | None = None,
) -> PathNode:
    return _append_node(
        state, frame, itid=itid, thread_name=thread_name,
        classification="unknown" if reason != "udk_irq" else "io_block",
        uncertainty=uncertainty or reason, termination_reason=reason,
    )


def _append_wakeup_edge(
    state: TraversalState, source: PathNode, waiting_node_id: int,
    waiter_itid: int, wakeup_ts: int,
) -> None:
    state.edges.append(PathEdge(
        edge_id=state.next_edge_id, from_node_id=source.node_id,
        to_node_id=waiting_node_id, from_itid=source.itid, to_itid=waiter_itid,
        parent_depth=source.depth - 1, child_depth=source.depth,
        wakeup_ts=wakeup_ts, edge_type="wakeup", confidence="fact",
        reason="sched_wakeup",
    ))
    state.next_edge_id += 1


def _append_terminal_for_dependency(
    state: TraversalState, waiter_itid: int, waker_itid: int,
    waiting_node_id: int, wakeup_ts: int, depth: int, reason: str,
    *, emit_edge: bool, blocking_context_node_id: int | None,
    inherited_blocked_caller: str | None,
) -> PathNode:
    node = _terminal_node(
        state, TraversalFrame(
            waker_itid, wakeup_ts, wakeup_ts, depth,
            blocking_context_node_id=blocking_context_node_id,
            inherited_blocked_caller=inherited_blocked_caller,
        ),
        reason, itid=waker_itid,
    )
    if emit_edge:
        _append_wakeup_edge(state, node, waiting_node_id, waiter_itid, wakeup_ts)
    return node


def _follow_waker(
    *, state: TraversalState, waiter_itid: int, waker_itid: int,
    wakeup_ts: int, waiting_node_id: int, frame_depth: int, max_depth: int,
    waker_name: str | None, blocking_context_node_id: int | None,
    inherited_blocked_caller: str | None, start_ts: int,
) -> TraversalFrame | None:
    key = (waiter_itid, waker_itid, wakeup_ts)
    if key in state.visited_wakeups:
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id, wakeup_ts,
            frame_depth + 1, "cycle_detected", emit_edge=False,
            blocking_context_node_id=blocking_context_node_id,
            inherited_blocked_caller=inherited_blocked_caller,
        )
        return None
    if frame_depth >= max_depth:
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id, wakeup_ts,
            frame_depth + 1, "max_depth", emit_edge=True,
            blocking_context_node_id=blocking_context_node_id,
            inherited_blocked_caller=inherited_blocked_caller,
        )
        return None
    if waker_name == "udk-irq":
        _append_terminal_for_dependency(
            state, waiter_itid, waker_itid, waiting_node_id, wakeup_ts,
            frame_depth + 1, "udk_irq", emit_edge=True,
            blocking_context_node_id=blocking_context_node_id,
            inherited_blocked_caller=inherited_blocked_caller,
        )
        return None
    state.visited_wakeups.add(key)
    return TraversalFrame(
        itid=waker_itid, start_ts=start_ts, end_ts=wakeup_ts,
        depth=frame_depth + 1, wakeup_target_node_id=waiting_node_id,
        blocking_context_node_id=blocking_context_node_id,
        inherited_blocked_caller=inherited_blocked_caller,
    )


def _process_frontier(facts: FactProvider, request: CriticalPathRequest, state: TraversalState) -> None:
    frame = state.frontier.pop()
    metadata = _fact_rows(
        facts.thread_metadata, "thread_metadata", frame.itid, frame.start_ts, frame.end_ts,
        frame.itid,
    )
    if metadata:
        state.metadata[frame.itid] = metadata[0]
    states = _fact_rows(
        facts.thread_state_segments, "thread_state_segments", frame.itid,
        frame.start_ts, frame.end_ts, frame.itid, frame.start_ts, frame.end_ts,
    )
    clipped = [
        (row, max(row["ts"], frame.start_ts), min(row["ts"] + row["dur"], frame.end_ts))
        for row in states
        if max(row["ts"], frame.start_ts) < min(row["ts"] + row["dur"], frame.end_ts)
    ]
    if not clipped:
        _terminal_node(state, frame, "missing_state")
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
        blocking_context_node_id=frame.blocking_context_node_id,
        inherited_blocked_caller=frame.inherited_blocked_caller,
        confidence="fact",
        uncertainty=uncertainty,
    )
    state.next_node_id += 1
    state.nodes.append(node)

    if frame.wakeup_target_node_id is not None:
        _append_wakeup_edge(
            state, node, frame.wakeup_target_node_id,
            state.nodes[frame.wakeup_target_node_id - 1].itid, frame.end_ts,
        )

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

    previous = None
    if state_name in {"R", "R+"}:
        candidates = [item for item in clipped if item[2] == segment_start]
        if candidates:
            previous = max(candidates, key=lambda item: (item[1], item[0]["state"]))

    if previous is not None and (
        previous[0]["state"] == "S" or previous[0]["state"].startswith("D")
    ):
        wait_row, wait_start, wait_end = previous
        wait_classification = _blocked_classification(wait_row["state"], wait_row)
        wait_node = PathNode(
            node_id=state.next_node_id, depth=frame.depth, itid=frame.itid,
            tid=metadata_row.get("tid"), thread_name=metadata_row.get("thread_name"),
            pid=metadata_row.get("pid"), process_name=metadata_row.get("process_name"),
            window_start_ts=frame.start_ts, window_end_ts=frame.end_ts,
            segment_start_ts=wait_start, segment_end_ts=wait_end,
            dur=wait_end - wait_start, state=wait_row["state"],
            classification=wait_classification, blocked_caller=wait_row.get("blocked_caller"),
            blocking_context_node_id=frame.blocking_context_node_id,
            inherited_blocked_caller=frame.inherited_blocked_caller,
            confidence="fact",
        )
        state.next_node_id += 1
        state.nodes.append(wait_node)
        state.edges.append(PathEdge(
            edge_id=state.next_edge_id, from_node_id=wait_node.node_id,
            to_node_id=node.node_id, from_itid=frame.itid, to_itid=frame.itid,
            parent_depth=frame.depth, child_depth=frame.depth,
            edge_type="sequence", confidence="fact", reason="thread_state_order",
        ))
        state.next_edge_id += 1

        wakeups = [row for row in _fact_rows(
            facts.wakeup_edges, "wakeup_edges", frame.itid, wait_start, segment_start,
            frame.itid, wait_start, segment_start,
        ) if row["target_itid"] == frame.itid and row["wakeup_ts"] <= segment_start]
        child = None
        if not wakeups:
            state.nodes[-1] = replace(
                wait_node, uncertainty="missing_waker", termination_reason="missing_waker"
            )
        else:
            latest_ts = max(row["wakeup_ts"] for row in wakeups)
            latest = [row for row in wakeups if row["wakeup_ts"] == latest_ts]
            wakers = {row["waker_itid"] for row in latest}
            if len(wakers) != 1:
                state.nodes[-1] = replace(wait_node, uncertainty="ambiguous_waker")
            else:
                waker_itid = next(iter(wakers))
                if wait_node.classification == "unknown":
                    state.nodes[-1] = replace(wait_node, classification="waiting_for_waker")
                waker_metadata = _fact_rows(
                    facts.thread_metadata, "thread_metadata", waker_itid,
                    frame.start_ts, latest_ts, waker_itid,
                )
                if waker_metadata:
                    state.metadata[waker_itid] = waker_metadata[0]
                waker_name = state.metadata.get(waker_itid, {}).get("thread_name")
                if waker_name in IO_THREAD_NAMES and waker_name not in IO_THREAD_EXCLUSIONS:
                    state.nodes[-1] = replace(state.nodes[-1], classification="io_block")
                blocking_node_id = None
                inherited_caller = None
                if wait_row["state"].startswith("D"):
                    blocking_node_id = wait_node.node_id
                    inherited_caller = wait_row.get("blocked_caller")
                child = _follow_waker(
                    state=state, waiter_itid=frame.itid, waker_itid=waker_itid,
                    wakeup_ts=latest_ts, waiting_node_id=wait_node.node_id,
                    frame_depth=frame.depth, max_depth=request.max_depth,
                    waker_name=waker_name,
                    blocking_context_node_id=blocking_node_id,
                    inherited_blocked_caller=inherited_caller,
                    start_ts=frame.start_ts,
                )
        if wait_start > frame.start_ts:
            state.frontier.append(TraversalFrame(
                itid=frame.itid, start_ts=frame.start_ts, end_ts=wait_start,
                depth=frame.depth, next_node_id=wait_node.node_id,
                blocking_context_node_id=frame.blocking_context_node_id,
                inherited_blocked_caller=frame.inherited_blocked_caller,
            ))
        if child is not None:
            state.frontier.append(child)
    elif segment_start > frame.start_ts:
        state.frontier.append(TraversalFrame(
            itid=frame.itid,
            start_ts=frame.start_ts,
            end_ts=segment_start,
            depth=frame.depth,
            next_node_id=node.node_id,
            blocking_context_node_id=frame.blocking_context_node_id,
            inherited_blocked_caller=frame.inherited_blocked_caller,
        ))


def _result(state: TraversalState) -> CriticalPathResult:
    return CriticalPathResult(
        nodes=_dataframe([asdict(node) for node in state.nodes], PATH_NODE_SCHEMA),
        edges=_dataframe([asdict(edge) for edge in state.edges], PATH_EDGE_SCHEMA),
    )


def _filter_short_segments(state: TraversalState, min_segment_ns: int) -> None:
    if min_segment_ns <= 0:
        return
    nodes_by_id = {node.node_id: node for node in state.nodes}
    protected_ids = {
        node_id
        for edge in state.edges
        if edge.edge_type == "wakeup"
        for node_id in (edge.from_node_id, edge.to_node_id)
    }
    for edge in state.edges:
        if edge.edge_type != "sequence":
            continue
        source = nodes_by_id.get(edge.from_node_id)
        target = nodes_by_id.get(edge.to_node_id)
        if source and target and (
            source.state == "S" or (source.state or "").startswith("D")
        ) and target.state in {"R", "R+"}:
            protected_ids.update((source.node_id, target.node_id))
    omitted_ids = {
        node.node_id
        for node in state.nodes
        if node.dur is not None
        and node.dur < min_segment_ns
        and node.node_id not in protected_ids
        and node.classification not in {"io_block", "non_io_block"}
        and not node.blocked_caller
        and not node.uncertainty
        and not node.termination_reason
    }
    if not omitted_ids:
        return
    state.nodes = [node for node in state.nodes if node.node_id not in omitted_ids]
    state.edges = [
        edge for edge in state.edges
        if edge.from_node_id not in omitted_ids and edge.to_node_id not in omitted_ids
    ]


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
    _filter_short_segments(state, int(request.min_segment_ms * 1_000_000))
    return _result(state)
