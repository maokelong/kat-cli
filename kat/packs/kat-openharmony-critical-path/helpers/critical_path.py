"""将 Trace Streamer 来源事实适配为 OpenHarmony 有界关键路径。"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass, field
from math import isfinite
from typing import Any, Protocol

import pyarrow as pa


CLOCK_DOMAIN = "boottime"
IO_WORKERS = {"fsverity", "cdecrypt", "erofs_unzipd", "fsignature", "hmfs", "wk:0/0/0", "wk:2/1/0", "wk:0/-20/0"}
BLOCKED_STATES = {"S", "D", "D-IO", "D-NIO"}
SCHEDULING_WAIT_STATES = {"R", "R+"}

# 帧定位成功时只发布一个 boottime 纳秒窗口。root_itid、start_ts 与 end_ts
# 是 extract-critical-path 的直接输入契约；callstack_id 是可选来源证据。
FRAME_WINDOW_SCHEMA = pa.schema([
    pa.field("frame_id", pa.int64(), nullable=False), pa.field("root_itid", pa.int64(), nullable=False),
    pa.field("start_ts", pa.int64(), nullable=False), pa.field("end_ts", pa.int64(), nullable=False),
    pa.field("duration_ns", pa.int64(), nullable=False), pa.field("process_id", pa.int64(), nullable=False),
    pa.field("process_name", pa.string(), nullable=False), pa.field("thread_id", pa.int64(), nullable=False),
    pa.field("thread_name", pa.string(), nullable=False), pa.field("callstack_id", pa.int64()),
    pa.field("clock_domain", pa.string(), nullable=False),
])
# 每行是一个观测到的 [start_ts, end_ts) 片段，时间单位是 clock_domain
# 指定的纳秒。segment_id 只在本 Output 内有效；parent_segment_id 指向下游，
# 根窗口的观测片段各自没有 parent；进入上游窗口后，wakeup 关系从直接
# 唤醒者指向下游，same_thread 关系从同一线程的较早片段指向较晚片段。segment_kind 只取
# execution、scheduling_wait 或 blocked。termination_reason 是确定终止边界：
# scheduling_wait、thread_exit、interrupt_boundary、min_segment_threshold、
# max_depth、cycle 或 missing_wakeup。uncertainty_reason 按稳定顺序以逗号连接
# incomplete_thread_state_coverage、missing_sched_coverage、
# missing_callstack_evidence、incomplete_callstack_coverage、
# missing_upstream_thread_state_coverage 或
# incomplete_upstream_thread_state_coverage。可空的调度和阻塞字段只在对应
# 来源事实存在时填写。
SEGMENT_SCHEMA = pa.schema([
    pa.field("segment_id", pa.int64(), nullable=False), pa.field("parent_segment_id", pa.int64()), pa.field("depth", pa.int64(), nullable=False),
    pa.field("clock_domain", pa.string(), nullable=False), pa.field("start_ts", pa.int64(), nullable=False), pa.field("end_ts", pa.int64(), nullable=False), pa.field("duration_ns", pa.int64(), nullable=False),
    pa.field("itid", pa.int64(), nullable=False), pa.field("tid", pa.int64(), nullable=False), pa.field("thread_name", pa.string(), nullable=False), pa.field("pid", pa.int64(), nullable=False), pa.field("process_name", pa.string(), nullable=False),
    pa.field("thread_state", pa.string(), nullable=False), pa.field("segment_kind", pa.string(), nullable=False), pa.field("relation_to_parent", pa.string(), nullable=False),
    pa.field("cpu", pa.int64()), pa.field("priority", pa.int64()), pa.field("io_wait", pa.int64()), pa.field("blocked_function", pa.string()),
    pa.field("termination_reason", pa.string()), pa.field("uncertainty_reason", pa.string()),
])
# 每行是一条裁剪后的来源调用栈证据，start_ts 与 end_ts 是对应片段
# boottime 纳秒域内的半开区间。证据通过 segment_id 连接片段；
# parent_callstack_id 保留来源关系，但可能指向裁剪后 Output 之外。
# business_category 只用于解释而不参与建边，取值为 application、runtime、
# io、interrupt 或 unknown。
CALLSTACK_SCHEMA = pa.schema([
    pa.field("segment_id", pa.int64(), nullable=False), pa.field("callstack_id", pa.int64(), nullable=False),
    pa.field("parent_callstack_id", pa.int64()), pa.field("callstack_depth", pa.int64(), nullable=False),
    pa.field("start_ts", pa.int64(), nullable=False), pa.field("end_ts", pa.int64(), nullable=False), pa.field("duration_ns", pa.int64(), nullable=False),
    pa.field("function_name", pa.string(), nullable=False), pa.field("business_category", pa.string(), nullable=False),
])


class CriticalPathError(RuntimeError):
    """来源事实无法安全解码时终止整个 Workflow。"""


class Facts(Protocol):
    def first_frame(self, process_name: str) -> dict[str, Any]: ...
    def metadata(self, itid: int) -> dict[str, Any]: ...
    def states(self, itid: int, start: int, end: int) -> list[dict[str, Any]]: ...
    def sched(self, itid: int, start: int, end: int) -> list[dict[str, Any]]: ...
    def callstacks(self, itid: int, start: int, end: int) -> list[dict[str, Any]]: ...
    def waker(self, itid: int, end: int) -> int | None: ...


class SourceAdapter(Protocol):
    def is_interrupt_boundary(self, metadata: dict[str, Any]) -> bool: ...
    def business_category(self, metadata: dict[str, Any], function: str, target_process: str) -> str: ...


@dataclass(frozen=True)
class OpenHarmonySourceAdapter:
    def is_interrupt_boundary(self, metadata: dict[str, Any]) -> bool:
        return _text(metadata, "thread_name", "thread") == "udk-irq"

    def business_category(self, metadata: dict[str, Any], function: str, target_process: str) -> str:
        name = _text(metadata, "thread_name", "thread")
        if self.is_interrupt_boundary(metadata):
            return "interrupt"
        if name in IO_WORKERS:
            return "io"
        if name.startswith("OS_FFRT_") or function.startswith("H:FFRT"):
            return "runtime"
        if _text(metadata, "process_name", "thread") == target_process:
            return "application"
        return "unknown"


class TraceStreamerFacts:
    def __init__(self, ctx: Any) -> None:
        self.ctx = ctx
        self.cache: dict[int, dict[str, Any]] = {}

    def first_frame(self, process_name: str) -> dict[str, Any]:
        rows = _rows(self.ctx.sql("""
            SELECT f.id AS frame_id, f.itid, f.ts, f.dur, f.callstack_id,
                   p.ipid, p.pid, arrow_cast(p.name, 'Utf8') AS process_name
            FROM process p JOIN frame_slice f ON f.ipid = p.ipid
            WHERE p.name = $process_name AND f.type = 0 AND f.dur > 0
            ORDER BY f.ts + f.dur, f.ts, f.id LIMIT 1
        """, params={"process_name": process_name}), "first actual frame")
        if not rows:
            raise ValueError(f"process_name {process_name!r} has no completed positive-duration actual frame")
        return rows[0]

    def metadata(self, itid: int) -> dict[str, Any]:
        if itid not in self.cache:
            rows = _rows(self.ctx.sql("""
                SELECT t.itid, t.ipid, t.tid, arrow_cast(t.name, 'Utf8') AS thread_name,
                       p.pid, arrow_cast(p.name, 'Utf8') AS process_name
                FROM thread t JOIN process p ON p.ipid = t.ipid
                WHERE t.itid = $itid LIMIT 2
            """, params={"itid": itid}), f"thread metadata for {itid}")
            if len(rows) != 1:
                raise CriticalPathError(f"thread metadata for itid {itid} must contain exactly one row")
            self.cache[itid] = rows[0]
        return self.cache[itid]

    def states(self, itid: int, start: int, end: int) -> list[dict[str, Any]]:
        rows = _rows(self.ctx.sql("""
            WITH states AS (
                SELECT itid, ts, dur, state, cpu, arg_setid FROM thread_state
                WHERE itid = $itid AND dur > 0 AND ts < $end_ts AND ts + dur > $start_ts
            ), decoded AS (
                SELECT a.argset,
                  MIN(CASE WHEN kd.data = 'iowait' THEN a.value END) AS io_wait_min,
                  MAX(CASE WHEN kd.data = 'iowait' THEN a.value END) AS io_wait_max,
                  MIN(CASE WHEN kd.data = 'caller' AND a.datatype = 1 THEN vd.data END) AS blocked_function_min,
                  MAX(CASE WHEN kd.data = 'caller' AND a.datatype = 1 THEN vd.data END) AS blocked_function_max
                FROM args a JOIN (SELECT DISTINCT arg_setid FROM states WHERE arg_setid IS NOT NULL) s ON s.arg_setid = a.argset
                JOIN data_dict kd ON kd.id = a.key LEFT JOIN data_dict vd ON vd.id = a.value GROUP BY a.argset
            ) SELECT s.itid, s.ts, s.dur, s.state, s.cpu, d.io_wait_min, d.io_wait_max,
                d.blocked_function_min, d.blocked_function_max
              FROM states s LEFT JOIN decoded d ON d.argset = s.arg_setid ORDER BY s.ts, s.dur, s.state
        """, params={"itid": itid, "start_ts": start, "end_ts": end}), f"thread states for {itid}")
        return _state_cover(rows, itid, start, end)

    def sched(self, itid: int, start: int, end: int) -> list[dict[str, Any]]:
        return _rows(self.ctx.sql("""
            SELECT ts, dur, cpu, priority FROM sched_slice
            WHERE itid = $itid AND dur > 0 AND ts < $end_ts AND ts + dur > $start_ts
            ORDER BY ts, dur, cpu
        """, params={"itid": itid, "start_ts": start, "end_ts": end}), f"sched slices for {itid}")

    def callstacks(self, itid: int, start: int, end: int) -> list[dict[str, Any]]:
        return _rows(self.ctx.sql("""
            SELECT id, parent_id, depth, ts, dur, arrow_cast(name, 'Utf8') AS function_name
            FROM callstack WHERE callid = $itid AND dur > 0 AND ts < $end_ts AND ts + dur > $start_ts
            ORDER BY ts, dur, depth, id
        """, params={"itid": itid, "start_ts": start, "end_ts": end}), f"callstacks for {itid}")

    def waker(self, itid: int, end: int) -> int | None:
        rows = _rows(self.ctx.sql("""
            SELECT DISTINCT wakeup_from FROM instant
            WHERE ref_type = 'itid' AND ref = $itid AND name LIKE 'sched_wakeup%'
              AND wakeup_from IS NOT NULL AND ts = $end_ts ORDER BY wakeup_from LIMIT 2
        """, params={"itid": itid, "end_ts": end}), f"wakeup facts for {itid}")
        if not rows:
            return None
        values = {_int(row, "wakeup_from", "wakeup fact") for row in rows}
        if len(values) != 1:
            raise CriticalPathError(f"wakeup for itid {itid} at {end} is ambiguous")
        return values.pop()


def locate_first_actual_frame(ctx: Any, process_name: str) -> Any:
    facts = TraceStreamerFacts(ctx)
    frame = facts.first_frame(process_name)
    metadata = facts.metadata(_int(frame, "itid", "frame"))
    if _int(metadata, "ipid", "thread") != _int(frame, "ipid", "frame"):
        raise CriticalPathError("frame root thread does not belong to the frame process")
    start = _int(frame, "ts", "frame", non_negative=True)
    duration = _int(frame, "dur", "frame", positive=True)
    return ctx.from_arrow(pa.Table.from_pylist([{
        "frame_id": _int(frame, "frame_id", "frame"), "root_itid": _int(frame, "itid", "frame"),
        "start_ts": start, "end_ts": _end(start, duration, "frame"), "duration_ns": duration,
        "process_id": _int(frame, "pid", "frame"), "process_name": _text(frame, "process_name", "frame"),
        "thread_id": _int(metadata, "tid", "thread"), "thread_name": _text(metadata, "thread_name", "thread"),
        "callstack_id": frame["callstack_id"], "clock_domain": CLOCK_DOMAIN,
    }], schema=FRAME_WINDOW_SCHEMA))


def extract_critical_path(ctx: Any, root_itid: int, start_ts: int, end_ts: int, max_depth: int = 8, min_segment_ms: float = 0.1) -> dict[str, Any]:
    if (
        type(root_itid) is not int
        or root_itid < 0
        or type(start_ts) is not int
        or start_ts < 0
        or type(end_ts) is not int
        or end_ts <= start_ts
    ):
        raise ValueError("root_itid, start_ts, and end_ts must define a non-negative, non-empty integer window")
    if type(max_depth) is not int or max_depth < 0:
        raise ValueError("max_depth must be a non-negative integer")
    if type(min_segment_ms) not in {int, float} or not isfinite(min_segment_ms) or min_segment_ms < 0:
        raise ValueError("min_segment_ms must be a non-negative finite number")
    walker = _Walker(TraceStreamerFacts(ctx), root_itid, start_ts, end_ts, max_depth, int(min_segment_ms * 1_000_000))
    segments, evidence = walker.run()
    return {"critical_path_segments": ctx.from_arrow(segments), "critical_path_callstack_evidence": ctx.from_arrow(evidence)}


@dataclass
class _Walker:
    facts: Facts
    root_itid: int
    start: int
    end: int
    max_depth: int
    min_duration_ns: int
    adapter: SourceAdapter = field(default_factory=OpenHarmonySourceAdapter)

    def __post_init__(self) -> None:
        self.rows: list[dict[str, Any]] = []
        self.evidence: list[dict[str, Any]] = []
        self.target_process = self.facts.metadata(self.root_itid)["process_name"]

    def run(self) -> tuple[pa.Table, pa.Table]:
        states = self.facts.states(self.root_itid, self.start, self.end)
        if not states:
            raise CriticalPathError(f"root itid {self.root_itid} has no state coverage")
        root_uncertainty = None
        if not _states_cover_window(states, self.start, self.end, self.root_itid):
            root_uncertainty = "incomplete_thread_state_coverage"
        for state in states:
            self._visit(self.root_itid, state, None, 0, "root", (), root_uncertainty)
        return pa.Table.from_pylist(self.rows, schema=SEGMENT_SCHEMA), pa.Table.from_pylist(self.evidence, schema=CALLSTACK_SCHEMA)

    def _visit(self, itid: int, state: dict[str, Any], parent: int | None, depth: int, relation: str, ancestry: tuple[int, ...], path_uncertainty: str | None = None) -> tuple[int, bool]:
        metadata = self.facts.metadata(itid)
        state_callstacks = self.facts.callstacks(itid, state["start"], state["end"])
        if state["state"] == "Running":
            spans = _execution_spans(state, self.facts.sched(itid, state["start"], state["end"]))
        else:
            spans = [(state["start"], state["end"], None)]
        current_parent = parent
        current_relation = relation
        earliest_segment_id = -1
        state_stops_path = False
        for start, end, sched in reversed(spans):
            callstacks = [row for row in state_callstacks if row["ts"] < end and _end(row["ts"], row["dur"], "callstack") > start]
            segment_id = len(self.rows)
            earliest_segment_id = segment_id
            kind = _kind(state["state"])
            termination = None
            uncertainty = _join_reasons(path_uncertainty, _uncertainty(kind, sched, callstacks, start, end))
            if kind == "scheduling_wait":
                termination = "scheduling_wait"
            elif state["state"] == "X":
                termination = "thread_exit"
            elif self.adapter.is_interrupt_boundary(metadata):
                termination = "interrupt_boundary"
            row = {
                "segment_id": segment_id, "parent_segment_id": current_parent, "depth": depth, "clock_domain": CLOCK_DOMAIN,
                "start_ts": start, "end_ts": end, "duration_ns": end - start, "itid": itid,
                "tid": _int(metadata, "tid", "thread"), "thread_name": _text(metadata, "thread_name", "thread"),
                "pid": _int(metadata, "pid", "thread"), "process_name": _text(metadata, "process_name", "thread"),
                "thread_state": state["state"], "segment_kind": kind, "relation_to_parent": current_relation,
                "cpu": None if sched is None else sched["cpu"], "priority": None if sched is None else sched["priority"],
                "io_wait": state["io_wait"], "blocked_function": state["blocked_function"],
                "termination_reason": termination, "uncertainty_reason": uncertainty,
            }
            self.rows.append(row)
            self._add_evidence(segment_id, metadata, start, end, callstacks)
            current_parent = segment_id
            current_relation = "same_thread"
            if termination or kind != "blocked":
                state_stops_path = state_stops_path or termination is not None or kind != "execution"
                continue
            state_stops_path = True
            if end - start < self.min_duration_ns:
                row["termination_reason"] = "min_segment_threshold"
            elif depth >= self.max_depth:
                row["termination_reason"] = "max_depth"
            elif itid in ancestry:
                row["termination_reason"] = "cycle"
            else:
                waker = self.facts.waker(itid, end)
                if waker is None:
                    row["termination_reason"] = "missing_wakeup"
                else:
                    upstream_states = self.facts.states(waker, start, end)
                    if not upstream_states:
                        row["uncertainty_reason"] = _join_reasons(
                            row["uncertainty_reason"],
                            "missing_upstream_thread_state_coverage",
                        )
                    elif not _states_cover_window(upstream_states, start, end, waker):
                        row["uncertainty_reason"] = _join_reasons(
                            row["uncertainty_reason"],
                            "incomplete_upstream_thread_state_coverage",
                        )
                    upstream_parent = segment_id
                    upstream_relation = "wakeup"
                    for upstream in reversed(upstream_states):
                        upstream_parent, upstream_stops_path = self._visit(
                            waker,
                            upstream,
                            upstream_parent,
                            depth + 1,
                            upstream_relation,
                            (*ancestry, itid),
                        )
                        upstream_relation = "same_thread"
                        if upstream_stops_path:
                            break
        return earliest_segment_id, state_stops_path

    def _add_evidence(self, segment_id: int, metadata: dict[str, Any], start: int, end: int, callstacks: list[dict[str, Any]]) -> None:
        for callstack in callstacks:
            evidence_start, evidence_end = max(start, callstack["ts"]), min(end, _end(callstack["ts"], callstack["dur"], "callstack"))
            if evidence_start >= evidence_end:
                continue
            function = _text(callstack, "function_name", "callstack")
            self.evidence.append({
                "segment_id": segment_id, "callstack_id": _int(callstack, "id", "callstack"), "parent_callstack_id": callstack["parent_id"],
                "callstack_depth": _int(callstack, "depth", "callstack"), "start_ts": evidence_start, "end_ts": evidence_end,
                "duration_ns": evidence_end - evidence_start, "function_name": function,
                "business_category": self.adapter.business_category(metadata, function, self.target_process),
            })


def _state_cover(rows: list[dict[str, Any]], itid: int, start: int, end: int) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []
    for row in rows:
        if _int(row, "itid", "thread state") != itid:
            raise CriticalPathError("thread state belongs to an unexpected thread")
        row_start = _int(row, "ts", "thread state")
        row_end = _end(row_start, _int(row, "dur", "thread state", positive=True), "thread state")
        io_wait = _consistent(row, "io_wait_min", "io_wait_max", "io_wait")
        function = _consistent(row, "blocked_function_min", "blocked_function_max", "blocked_function")
        clipped_start, clipped_end = max(start, row_start), min(end, row_end)
        if clipped_start < clipped_end:
            result.append({"start": clipped_start, "end": clipped_end, "state": _text(row, "state", "thread state"), "io_wait": io_wait, "blocked_function": function})
    result.sort(key=lambda row: (row["start"], row["end"], row["state"]))
    return result


def _execution_spans(state: dict[str, Any], sched: list[dict[str, Any]]) -> list[tuple[int, int, dict[str, Any] | None]]:
    boundaries = {state["start"], state["end"]}
    for row in sched:
        boundaries.add(max(state["start"], row["ts"])); boundaries.add(min(state["end"], _end(row["ts"], row["dur"], "evidence")))
    points = sorted(boundary for boundary in boundaries if state["start"] <= boundary <= state["end"])
    result = []
    for start, end in zip(points, points[1:]):
        if start >= end:
            continue
        matching_sched = [row for row in sched if row["ts"] <= start and _end(row["ts"], row["dur"], "sched slice") >= end]
        if len(matching_sched) > 1:
            raise CriticalPathError(f"conflicting sched slices cover [{start}, {end})")
        result.append((start, end, matching_sched[0] if matching_sched else None))
    return result


def _kind(state: str) -> str:
    if state == "Running": return "execution"
    if state in SCHEDULING_WAIT_STATES: return "scheduling_wait"
    if state in BLOCKED_STATES: return "blocked"
    if state == "X": return "blocked"
    raise CriticalPathError(f"unsupported thread state {state!r}")


def _uncertainty(kind: str, sched: dict[str, Any] | None, callstacks: list[dict[str, Any]], start: int, end: int) -> str | None:
    missing = []
    if kind == "execution" and sched is None: missing.append("missing_sched_coverage")
    if not callstacks:
        missing.append("missing_callstack_evidence")
    elif not _covers_window(callstacks, start, end, "callstack"):
        missing.append("incomplete_callstack_coverage")
    return ",".join(missing) or None


def _covers_window(rows: list[dict[str, Any]], start: int, end: int, label: str) -> bool:
    intervals = []
    for row in rows:
        row_end = _end(row["ts"], row["dur"], label)
        if row["ts"] < end and row_end > start:
            intervals.append((max(start, row["ts"]), min(end, row_end)))
    return _covers_intervals(intervals, start, end)


def _states_cover_window(states: list[dict[str, Any]], start: int, end: int, itid: int) -> bool:
    intervals = sorted(
        (max(start, state["start"]), min(end, state["end"]))
        for state in states
    )
    covered_until = start
    complete = True
    for interval_start, interval_end in intervals:
        if interval_start >= interval_end:
            raise CriticalPathError(f"invalid thread state interval for itid {itid}")
        if interval_start < covered_until:
            raise CriticalPathError(f"overlapping thread states for itid {itid}")
        if interval_start > covered_until:
            complete = False
        covered_until = interval_end
    return complete and covered_until >= end


def _covers_intervals(intervals: Iterable[tuple[int, int]], start: int, end: int) -> bool:
    covered_until = start
    for interval_start, interval_end in sorted(intervals):
        if interval_start > covered_until:
            return False
        covered_until = max(covered_until, interval_end)
        if covered_until >= end:
            return True
    return False


def _join_reasons(*reasons: str | None) -> str | None:
    values = [reason for reason in reasons if reason]
    return ",".join(values) or None


def _rows(frame: Any, label: str) -> list[dict[str, Any]]:
    try:
        return frame.to_rows()
    except Exception as error:
        raise CriticalPathError(f"failed to read {label}") from error


def _int(row: dict[str, Any], name: str, label: str, *, positive: bool = False, non_negative: bool = False) -> int:
    value = row[name]
    if type(value) is not int or (positive and value <= 0) or (non_negative and value < 0):
        qualifier = "positive " if positive else "non-negative " if non_negative else ""
        raise CriticalPathError(f"{label} {name} must be a {qualifier}integer")
    return value


def _text(row: dict[str, Any], name: str, label: str) -> str:
    value = row[name]
    if type(value) is not str or not value: raise CriticalPathError(f"{label} {name} must be a non-empty string")
    return value


def _end(start: int, duration: int, label: str) -> int:
    if type(start) is not int or start < 0 or type(duration) is not int or duration <= 0:
        raise CriticalPathError(f"{label} interval must use a non-negative start and positive integer duration")
    end = start + duration
    if end <= start or end > 2**63 - 1: raise CriticalPathError(f"{label} interval is invalid")
    return end


def _consistent(row: dict[str, Any], minimum: str, maximum: str, label: str) -> Any:
    if row[minimum] != row[maximum]: raise CriticalPathError(f"{label} has conflicting values")
    return row[minimum]
