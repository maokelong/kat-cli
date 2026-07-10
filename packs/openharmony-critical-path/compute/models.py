from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable

import pyarrow as pa


@dataclass(frozen=True)
class CriticalPathRequest:
    root_itid: int
    start_ts: int
    end_ts: int
    max_depth: int = 8
    min_segment_ms: float = 0.1


@dataclass(frozen=True)
class FactProvider:
    thread_metadata: Callable[[int], Any]
    thread_state_segments: Callable[[int, int, int], Any]
    wakeup_edges: Callable[[int, int, int], Any]
    sched_slices: Callable[[int, int, int], Any]
    callstack_slices: Callable[[int, int, int], Any]


@dataclass(frozen=True)
class TraversalFrame:
    itid: int
    start_ts: int
    end_ts: int
    depth: int
    wakeup_target_node_id: int | None = None
    blocking_context_node_id: int | None = None
    inherited_blocked_caller: str | None = None
    next_node_id: int | None = None


@dataclass(frozen=True)
class PathNode:
    node_id: int
    depth: int
    itid: int | None = None
    tid: int | None = None
    thread_name: str | None = None
    pid: int | None = None
    process_name: str | None = None
    window_start_ts: int | None = None
    window_end_ts: int | None = None
    segment_start_ts: int | None = None
    segment_end_ts: int | None = None
    dur: int | None = None
    state: str | None = None
    classification: str | None = None
    sched_cpu: int | None = None
    sched_priority: int | None = None
    callstack_name: str | None = None
    blocked_caller: str | None = None
    blocking_context_node_id: int | None = None
    inherited_blocked_caller: str | None = None
    confidence: str | None = None
    uncertainty: str | None = None
    termination_reason: str | None = None


@dataclass(frozen=True)
class PathEdge:
    edge_id: int
    from_node_id: int
    to_node_id: int
    from_itid: int | None = None
    to_itid: int | None = None
    parent_depth: int | None = None
    child_depth: int | None = None
    wakeup_ts: int | None = None
    edge_type: str | None = None
    confidence: str | None = None
    reason: str | None = None


@dataclass
class TraversalState:
    frontier: list[TraversalFrame] = field(default_factory=list)
    visited_wakeups: set[tuple[int, int, int]] = field(default_factory=set)
    nodes: list[PathNode] = field(default_factory=list)
    edges: list[PathEdge] = field(default_factory=list)
    metadata: dict[int, dict[str, Any]] = field(default_factory=dict)
    next_node_id: int = 1
    next_edge_id: int = 1


@dataclass(frozen=True)
class CriticalPathResult:
    nodes: Any
    edges: Any


PATH_NODE_SCHEMA = pa.schema([
    ("node_id", pa.int64()), ("depth", pa.int64()), ("itid", pa.int64()),
    ("tid", pa.int64()), ("thread_name", pa.string()), ("pid", pa.int64()),
    ("process_name", pa.string()), ("window_start_ts", pa.int64()),
    ("window_end_ts", pa.int64()), ("segment_start_ts", pa.int64()),
    ("segment_end_ts", pa.int64()), ("dur", pa.int64()), ("state", pa.string()),
    ("classification", pa.string()), ("sched_cpu", pa.int64()),
    ("sched_priority", pa.int64()), ("callstack_name", pa.string()),
    ("blocked_caller", pa.string()), ("blocking_context_node_id", pa.int64()),
    ("inherited_blocked_caller", pa.string()), ("confidence", pa.string()),
    ("uncertainty", pa.string()), ("termination_reason", pa.string()),
])

PATH_EDGE_SCHEMA = pa.schema([
    ("edge_id", pa.int64()), ("from_node_id", pa.int64()), ("to_node_id", pa.int64()),
    ("from_itid", pa.int64()), ("to_itid", pa.int64()), ("parent_depth", pa.int64()),
    ("child_depth", pa.int64()), ("wakeup_ts", pa.int64()), ("edge_type", pa.string()),
    ("confidence", pa.string()), ("reason", pa.string()),
])
