from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class ThreadRef:
    itid: int
    tid: int | None = None
    name: str = ""
    pid: int | None = None
    process_name: str = ""


@dataclass(frozen=True)
class StateSegment:
    id: int | None
    itid: int
    ts: int
    dur: int
    state: str
    blocked_function: str = ""
    final_blocked_caller: str = ""
    io_wait: bool = False

    @property
    def end_ts(self) -> int:
        return self.ts + self.dur

    def overlap(self, start_ts: int, end_ts: int) -> int:
        start = max(self.ts, start_ts)
        end = min(self.end_ts, end_ts)
        return max(0, end - start)


@dataclass(frozen=True)
class WakeupEdge:
    id: int | None
    ts: int
    target_itid: int
    waker_itid: int | None
    name: str = "sched_wakeup"


@dataclass(frozen=True)
class CallstackSlice:
    id: int | None
    itid: int
    ts: int
    dur: int
    name: str
    depth: int | None = None
    parent_id: int | None = None

    @property
    def end_ts(self) -> int:
        return self.ts + self.dur

    def overlap(self, start_ts: int, end_ts: int) -> int:
        start = max(self.ts, start_ts)
        end = min(self.end_ts, end_ts)
        return max(0, end - start)


@dataclass
class TraceFacts:
    threads: dict[int, ThreadRef] = field(default_factory=dict)
    states: list[StateSegment] = field(default_factory=list)
    wakeups: list[WakeupEdge] = field(default_factory=list)
    callstacks: list[CallstackSlice] = field(default_factory=list)

    def states_for(self, itid: int, start_ts: int, end_ts: int, min_segment_ns: int) -> list[StateSegment]:
        rows = [
            state
            for state in self.states
            if state.itid == itid
            and state.ts < end_ts
            and state.end_ts > start_ts
            and state.overlap(start_ts, end_ts) >= min_segment_ns
        ]
        return sorted(rows, key=lambda state: (state.ts, state.id or -1))

    def wakeup_for(self, target_itid: int, start_ts: int, end_ts: int) -> WakeupEdge | None:
        rows = [
            edge
            for edge in self.wakeups
            if edge.target_itid == target_itid
            and edge.waker_itid is not None
            and start_ts <= edge.ts <= end_ts
        ]
        if not rows:
            return None
        return sorted(rows, key=lambda edge: (edge.ts, edge.id or -1))[-1]

    def best_callstack(self, itid: int, start_ts: int, end_ts: int) -> CallstackSlice | None:
        rows = [
            row
            for row in self.callstacks
            if row.itid == itid and row.ts < end_ts and row.end_ts > start_ts
        ]
        if not rows:
            return None
        return sorted(
            rows,
            key=lambda row: (
                -row.overlap(start_ts, end_ts),
                row.depth if row.depth is not None else 1_000_000,
                row.ts,
                row.id or -1,
            ),
        )[0]

    def thread_name(self, itid: int | None) -> str:
        if itid is None:
            return ""
        thread = self.threads.get(itid)
        return thread.name if thread else ""


@dataclass(frozen=True)
class AnalysisWindow:
    root_itid: int
    start_ts: int
    end_ts: int


@dataclass(frozen=True)
class PathNode:
    node_id: int
    depth: int
    itid: int
    tid: int | None
    thread_name: str
    start_ts: int
    end_ts: int
    state: str
    classification: str
    reason: str
    blocked_context: str = ""
    evidence_name: str = ""
    source_state_id: int | None = None

    @property
    def duration_ns(self) -> int:
        return max(0, self.end_ts - self.start_ts)


@dataclass(frozen=True)
class PathEdge:
    edge_id: int
    relation: str
    from_node_id: int | None
    to_node_id: int | None
    from_itid: int | None
    to_itid: int | None
    start_ts: int
    end_ts: int
    classification: str
    evidence_name: str = ""
    source_wakeup_id: int | None = None

    @property
    def duration_ns(self) -> int:
        return max(0, self.end_ts - self.start_ts)


@dataclass(frozen=True)
class Uncertainty:
    code: str
    message: str
    itid: int | None
    depth: int
    start_ts: int
    end_ts: int


@dataclass
class CriticalPathResult:
    window: AnalysisWindow
    nodes: list[PathNode] = field(default_factory=list)
    edges: list[PathEdge] = field(default_factory=list)
    uncertainties: list[Uncertainty] = field(default_factory=list)

    def classification_rows(self) -> list[dict[str, Any]]:
        rows: dict[str, dict[str, Any]] = {}
        for node in self.nodes:
            row = rows.setdefault(
                node.classification,
                {
                    "classification": node.classification,
                    "node_count": 0,
                    "total_duration_ns": 0,
                    "max_depth": 0,
                },
            )
            row["node_count"] += 1
            row["total_duration_ns"] += node.duration_ns
            row["max_depth"] = max(row["max_depth"], node.depth)
        return sorted(rows.values(), key=lambda row: (-row["total_duration_ns"], row["classification"]))

    def evidence_rows(self) -> list[dict[str, Any]]:
        return [
            {
                "fact_kind": "path_shape",
                "node_count": len(self.nodes),
                "edge_count": len(self.edges),
                "uncertainty_count": len(self.uncertainties),
                "max_depth": max((node.depth for node in self.nodes), default=0),
            }
        ]
