from __future__ import annotations

from dataclasses import dataclass

from .classify import blocked_context, classify_state, is_io_thread, is_irq_thread, normalized_state
from .model import (
    AnalysisWindow,
    CriticalPathResult,
    PathEdge,
    PathNode,
    StateSegment,
    TraceFacts,
    Uncertainty,
    WakeupEdge,
)


@dataclass
class _Counters:
    node_id: int = 0
    edge_id: int = 0

    def next_node_id(self) -> int:
        self.node_id += 1
        return self.node_id

    def next_edge_id(self) -> int:
        self.edge_id += 1
        return self.edge_id


def analyze_critical_path(
    facts: TraceFacts,
    root_itid: int,
    start_ts: int,
    end_ts: int,
    max_depth: int = 8,
    min_segment_ns: int = 100_000,
) -> CriticalPathResult:
    if start_ts >= end_ts:
        raise ValueError("start_ts must be smaller than end_ts")
    if max_depth < 0:
        raise ValueError("max_depth must be non-negative")
    if min_segment_ns <= 0:
        raise ValueError("min_segment_ns must be positive")

    result = CriticalPathResult(window=AnalysisWindow(root_itid=root_itid, start_ts=start_ts, end_ts=end_ts))
    _analyze_thread(
        facts=facts,
        result=result,
        counters=_Counters(),
        itid=root_itid,
        start_ts=start_ts,
        end_ts=end_ts,
        depth=0,
        max_depth=max_depth,
        min_segment_ns=min_segment_ns,
        inherited_block_context="",
        seen_edges=set(),
        active_itids=(root_itid,),
    )
    return result


def _analyze_thread(
    facts: TraceFacts,
    result: CriticalPathResult,
    counters: _Counters,
    itid: int,
    start_ts: int,
    end_ts: int,
    depth: int,
    max_depth: int,
    min_segment_ns: int,
    inherited_block_context: str,
    seen_edges: set[tuple[int, int, int]],
    active_itids: tuple[int, ...],
) -> list[int]:
    if depth > max_depth:
        result.uncertainties.append(
            Uncertainty(
                code="max_depth_reached",
                message="dependency recursion stopped at max_depth",
                itid=itid,
                depth=depth,
                start_ts=start_ts,
                end_ts=end_ts,
            )
        )
        return []

    states = facts.states_for(itid, start_ts, end_ts, min_segment_ns)
    if not states:
        result.uncertainties.append(
            Uncertainty(
                code="missing_state",
                message="no state segment covers the requested window",
                itid=itid,
                depth=depth,
                start_ts=start_ts,
                end_ts=end_ts,
            )
        )
        return []

    node_ids: list[int] = []
    previous_node_id: int | None = None
    for state in states:
        node = _make_node(facts, counters, state, start_ts, end_ts, depth, inherited_block_context)
        result.nodes.append(node)
        node_ids.append(node.node_id)

        if previous_node_id is not None:
            result.edges.append(
                PathEdge(
                    edge_id=counters.next_edge_id(),
                    relation="prev_next",
                    from_node_id=previous_node_id,
                    to_node_id=node.node_id,
                    from_itid=itid,
                    to_itid=itid,
                    start_ts=node.start_ts,
                    end_ts=node.end_ts,
                    classification="time_order",
                    evidence_name="state_order",
                )
            )
        previous_node_id = node.node_id

        _follow_dependency(
            facts=facts,
            result=result,
            counters=counters,
            node=node,
            state=state,
            root_start_ts=start_ts,
            root_end_ts=end_ts,
            depth=depth,
            max_depth=max_depth,
            min_segment_ns=min_segment_ns,
            seen_edges=seen_edges,
            active_itids=active_itids,
        )

    return node_ids


def _make_node(
    facts: TraceFacts,
    counters: _Counters,
    state: StateSegment,
    start_ts: int,
    end_ts: int,
    depth: int,
    inherited_block_context: str,
) -> PathNode:
    thread = facts.threads.get(state.itid)
    classification, reason = classify_state(
        state.state,
        state.blocked_function,
        state.final_blocked_caller,
        state.io_wait,
    )
    local_block_context = blocked_context(state.blocked_function, state.final_blocked_caller)
    evidence = facts.best_callstack(state.itid, max(state.ts, start_ts), min(state.end_ts, end_ts))
    return PathNode(
        node_id=counters.next_node_id(),
        depth=depth,
        itid=state.itid,
        tid=thread.tid if thread else None,
        thread_name=thread.name if thread else "",
        start_ts=max(state.ts, start_ts),
        end_ts=min(state.end_ts, end_ts),
        state=state.state,
        classification=classification,
        reason=reason,
        blocked_context=local_block_context or inherited_block_context,
        evidence_name=evidence.name if evidence else "",
        source_state_id=state.id,
    )


def _follow_dependency(
    facts: TraceFacts,
    result: CriticalPathResult,
    counters: _Counters,
    node: PathNode,
    state: StateSegment,
    root_start_ts: int,
    root_end_ts: int,
    depth: int,
    max_depth: int,
    min_segment_ns: int,
    seen_edges: set[tuple[int, int, int]],
    active_itids: tuple[int, ...],
) -> None:
    kind = normalized_state(state.state)
    if kind == "running":
        return

    wakeup = facts.wakeup_for(state.itid, max(root_start_ts, state.ts), min(root_end_ts, state.end_ts))
    if wakeup is None or wakeup.waker_itid is None:
        result.uncertainties.append(
            Uncertainty(
                code="missing_waker",
                message="no wakeup source closes this waiting segment",
                itid=state.itid,
                depth=depth,
                start_ts=max(state.ts, root_start_ts),
                end_ts=min(state.end_ts, root_end_ts),
            )
        )
        return

    waker_name = facts.thread_name(wakeup.waker_itid)
    dependency_classification = _dependency_classification(wakeup, waker_name, state)
    if is_irq_thread(waker_name):
        result.edges.append(
            PathEdge(
                edge_id=counters.next_edge_id(),
                relation="upper_lower",
                from_node_id=node.node_id,
                to_node_id=None,
                from_itid=state.itid,
                to_itid=wakeup.waker_itid,
                start_ts=max(root_start_ts, state.ts),
                end_ts=wakeup.ts,
                classification=dependency_classification,
                evidence_name=wakeup.name,
                source_wakeup_id=wakeup.id,
            )
        )
        result.uncertainties.append(
            Uncertainty(
                code="irq_cutoff",
                message="wakeup source is udk-irq; recursion stopped to avoid interrupt fan-out",
                itid=wakeup.waker_itid,
                depth=depth + 1,
                start_ts=max(root_start_ts, state.ts),
                end_ts=wakeup.ts,
            )
        )
        return

    edge_key = (state.itid, wakeup.waker_itid, wakeup.ts)
    if edge_key in seen_edges:
        result.uncertainties.append(
            Uncertainty(
                code="cycle_detected",
                message="dependency edge repeated; recursion stopped",
                itid=wakeup.waker_itid,
                depth=depth + 1,
                start_ts=max(root_start_ts, state.ts),
                end_ts=wakeup.ts,
            )
        )
        return
    if wakeup.waker_itid in active_itids:
        result.uncertainties.append(
            Uncertainty(
                code="cycle_detected",
                message="dependency thread is already active in this path; recursion stopped",
                itid=wakeup.waker_itid,
                depth=depth + 1,
                start_ts=max(root_start_ts, state.ts),
                end_ts=wakeup.ts,
            )
        )
        return

    dependency_start_ts = max(root_start_ts, min(state.ts, wakeup.ts))
    dependency_end_ts = min(root_end_ts, wakeup.ts)
    if dependency_start_ts >= dependency_end_ts:
        result.uncertainties.append(
            Uncertainty(
                code="empty_dependency_window",
                message="wakeup edge has no analyzable dependency window",
                itid=wakeup.waker_itid,
                depth=depth + 1,
                start_ts=dependency_start_ts,
                end_ts=dependency_end_ts,
            )
        )
        return

    next_seen = set(seen_edges)
    next_seen.add(edge_key)
    child_node_ids = _analyze_thread(
        facts=facts,
        result=result,
        counters=counters,
        itid=wakeup.waker_itid,
        start_ts=dependency_start_ts,
        end_ts=dependency_end_ts,
        depth=depth + 1,
        max_depth=max_depth,
        min_segment_ns=min_segment_ns,
        inherited_block_context=node.blocked_context,
        seen_edges=next_seen,
        active_itids=active_itids + (wakeup.waker_itid,),
    )
    result.edges.append(
        PathEdge(
            edge_id=counters.next_edge_id(),
            relation="upper_lower",
            from_node_id=node.node_id,
            to_node_id=child_node_ids[0] if child_node_ids else None,
            from_itid=state.itid,
            to_itid=wakeup.waker_itid,
            start_ts=max(root_start_ts, state.ts),
            end_ts=wakeup.ts,
            classification=dependency_classification,
            evidence_name=wakeup.name,
            source_wakeup_id=wakeup.id,
        )
    )


def _dependency_classification(wakeup: WakeupEdge, waker_name: str, state: StateSegment) -> str:
    if is_irq_thread(waker_name) or is_io_thread(waker_name) or state.io_wait:
        return "io_block"
    if normalized_state(state.state) == "runnable":
        return "wakeup_dependency"
    if blocked_context(state.blocked_function, state.final_blocked_caller):
        return "non_io_block"
    return "wakeup_dependency"
