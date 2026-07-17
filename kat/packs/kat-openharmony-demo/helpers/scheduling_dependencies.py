"""Narrow OpenHarmony adapter for Perfetto 0621e927 scheduling semantics.

Trace Streamer states and wakeups form a lazily read wakeup graph. The blocker
walker clips every state and graph edge to the selected frame window; this PACK
does not embed Perfetto Runtime or expose the private graph.
"""

from __future__ import annotations

from typing import Any

import pyarrow as pa


_CLOCK_DOMAIN = "boottime"
_SELF_OWNED_STATES = {"Running", "R", "R+"}
_MAX_STATE_ROWS = 4096
_OPENHARMONY_IO_WORKER_RULES = {
    "fsverity": True,
    "cdecrypt": True,
    "erofs_unzipd": True,
    "fsignature": True,
    "hmfs": True,
    "wk:0/0/0": True,
    "wk:2/1/0": True,
    "wk:0/-20/0": True,
    "hmfs_txn": False,
}

OUTPUT_SCHEMA = pa.schema(
    [
        pa.field("clock_domain", pa.string(), nullable=False),
        pa.field("clock_value", pa.int64(), nullable=False),
        pa.field("duration_ns", pa.int64(), nullable=False),
        pa.field("frame_thread_id", pa.int64(), nullable=False),
        pa.field("frame_thread_name", pa.string(), nullable=False),
        pa.field("frame_thread_state", pa.string(), nullable=False),
        pa.field("frame_io_wait", pa.int64()),
        pa.field("frame_blocked_function", pa.string()),
        pa.field("blocker_thread_id", pa.int64(), nullable=False),
        pa.field("blocker_thread_name", pa.string(), nullable=False),
        pa.field("blocker_process_id", pa.int64(), nullable=False),
        pa.field("blocker_process_name", pa.string(), nullable=False),
        pa.field("blocker_thread_state", pa.string(), nullable=False),
        pa.field("blocker_cpu", pa.int64()),
        pa.field("blocker_io_wait", pa.int64()),
        pa.field("blocker_blocked_function", pa.string()),
    ]
)


class AttributionError(RuntimeError):
    """The selected frame cannot be completely attributed from source facts."""


class _Facts:
    def __init__(self, ctx: Any) -> None:
        self._ctx = ctx
        self._metadata: dict[int, dict[str, Any]] = {}

    def target(self, process_name: str) -> dict[str, Any]:
        rows = _rows(
            self._ctx.sql(
                """
                SELECT
                    p.ipid,
                    f.itid,
                    f.ts,
                    f.dur
                FROM process p
                LEFT JOIN frame_slice f
                  ON f.ipid = p.ipid
                 AND f.type = 0
                 AND f.dur > 0
                WHERE p.name = $process_name
                ORDER BY f.ts ASC NULLS LAST, p.ipid ASC, f.itid ASC
                LIMIT 1
                """,
                process_name=process_name,
            ),
            "target frame",
            max_rows=1,
        )
        if not rows:
            raise ValueError(f"process_name {process_name!r} was not found")
        row = rows[0]
        if row["itid"] is None:
            raise ValueError(
                f"process_name {process_name!r} has no completed positive-duration actual frame"
            )
        _require_int(row, "itid", "target frame")
        _require_int(row, "ts", "target frame")
        _require_int(row, "dur", "target frame", positive=True)
        return row

    def metadata(self, itid: int) -> dict[str, Any]:
        if itid in self._metadata:
            return self._metadata[itid]
        rows = _rows(
            self._ctx.sql(
                """
                SELECT
                    t.itid,
                    t.tid,
                    arrow_cast(t.name, 'Utf8') AS thread_name,
                    p.pid,
                    arrow_cast(p.name, 'Utf8') AS process_name
                FROM thread t
                JOIN process p ON p.ipid = t.ipid
                WHERE t.itid = $itid
                LIMIT 2
                """,
                itid=itid,
            ),
            f"thread metadata for itid {itid}",
            max_rows=2,
        )
        if len(rows) != 1:
            raise AttributionError(
                f"thread metadata for itid {itid} must contain exactly one complete row"
            )
        row = rows[0]
        for name in ("itid", "tid", "pid"):
            _require_int(row, name, "thread metadata")
        for name in ("thread_name", "process_name"):
            if type(row[name]) is not str or not row[name]:
                raise AttributionError(f"thread metadata has invalid {name}")
        self._metadata[itid] = row
        return row

    def states(self, itid: int, start: int, end: int) -> list[dict[str, Any]]:
        rows = _rows(
            self._ctx.sql(
                """
                WITH requested_states AS (
                    SELECT itid, ts, dur, state, cpu, arg_setid
                    FROM thread_state
                    WHERE itid = $itid
                      AND dur > 0
                      AND ts < $end_ts
                      AND ts + dur > $start_ts
                ), requested_argsets AS (
                    SELECT DISTINCT arg_setid
                    FROM requested_states
                    WHERE arg_setid IS NOT NULL
                ), decoded AS (
                    SELECT
                        a.argset,
                        MAX(CASE WHEN key_dict.data = 'iowait' THEN a.value END)
                            AS io_wait,
                        MAX(CASE
                            WHEN key_dict.data = 'caller' AND a.datatype = 1
                            THEN value_dict.data
                        END) AS blocked_function
                    FROM args a
                    JOIN requested_argsets requested
                      ON requested.arg_setid = a.argset
                    JOIN data_dict key_dict ON key_dict.id = a.key
                    LEFT JOIN data_dict value_dict ON value_dict.id = a.value
                    GROUP BY a.argset
                )
                SELECT
                    s.itid,
                    s.ts,
                    s.dur,
                    s.state AS thread_state_name,
                    s.cpu,
                    decoded.io_wait,
                    decoded.blocked_function AS decoded_blocked_function
                FROM requested_states s
                LEFT JOIN decoded ON decoded.argset = s.arg_setid
                ORDER BY s.ts ASC, s.dur ASC, s.state ASC
                LIMIT 4097
                """,
                itid=itid,
                start_ts=start,
                end_ts=end,
            ),
            f"thread states for itid {itid}",
            max_rows=_MAX_STATE_ROWS,
        )
        return _complete_state_cover(rows, itid, start, end)

    def waker(self, itid: int, start: int, end: int) -> int:
        rows = _rows(
            self._ctx.sql(
                """
                WITH requested AS (
                    SELECT
                        ts AS wakeup_ts,
                        ref AS target_itid,
                        wakeup_from AS waker_itid
                    FROM instant
                    WHERE ref_type = 'itid'
                      AND name LIKE 'sched_wakeup%'
                      AND ref = $itid
                      AND wakeup_from IS NOT NULL
                      AND ts > $start_ts
                      AND ts <= $end_ts
                ), latest AS (
                    SELECT MAX(wakeup_ts) AS wakeup_ts
                    FROM requested
                )
                SELECT DISTINCT
                    requested.wakeup_ts,
                    requested.target_itid,
                    requested.waker_itid
                FROM requested
                JOIN latest ON latest.wakeup_ts = requested.wakeup_ts
                ORDER BY requested.waker_itid ASC
                LIMIT 2
                """,
                itid=itid,
                start_ts=start,
                end_ts=end,
            ),
            f"wakeup facts for itid {itid}",
            max_rows=2,
        )
        if not rows:
            raise AttributionError(
                f"blocked interval [{start}, {end}) for itid {itid} has no waker"
            )
        latest = max(_require_int(row, "wakeup_ts", "wakeup fact") for row in rows)
        if latest != end:
            raise AttributionError(
                f"blocked interval [{start}, {end}) for itid {itid} is not closed by its waker"
            )
        wakers = {
            _require_int(row, "waker_itid", "wakeup fact")
            for row in rows
            if row["wakeup_ts"] == latest
        }
        if len(wakers) != 1:
            raise AttributionError(
                f"blocked interval [{start}, {end}) for itid {itid} has ambiguous wakers"
            )
        return next(iter(wakers))


def analyze_first_frame(ctx: Any, process_name: str) -> Any:
    facts = _Facts(ctx)
    target = facts.target(process_name)
    root_itid = target["itid"]
    start = target["ts"]
    end = _checked_end(start, target["dur"], "target frame")
    frame_metadata = facts.metadata(root_itid)
    rows: list[dict[str, Any]] = []

    for frame_state in facts.states(root_itid, start, end):
        for blocker in _attribute(
            facts,
            root_itid,
            frame_state["start"],
            frame_state["end"],
            node_path=(),
            before_node_ts=None,
        ):
            rows.append(
                {
                    "clock_domain": _CLOCK_DOMAIN,
                    "clock_value": blocker["start"],
                    "duration_ns": blocker["end"] - blocker["start"],
                    "frame_thread_id": frame_metadata["tid"],
                    "frame_thread_name": frame_metadata["thread_name"],
                    "frame_thread_state": frame_state["state"],
                    "frame_io_wait": frame_state["io_wait"],
                    "frame_blocked_function": frame_state["blocked_function"],
                    "blocker_thread_id": blocker["metadata"]["tid"],
                    "blocker_thread_name": blocker["metadata"]["thread_name"],
                    "blocker_process_id": blocker["metadata"]["pid"],
                    "blocker_process_name": blocker["metadata"]["process_name"],
                    "blocker_thread_state": blocker["state"],
                    "blocker_cpu": blocker["cpu"],
                    "blocker_io_wait": blocker["io_wait"],
                    "blocker_blocked_function": blocker["blocked_function"],
                }
            )

    _validate_output_cover(rows, start, end)
    return ctx.from_arrow(pa.Table.from_pylist(rows, schema=OUTPUT_SCHEMA))


def _attribute(
    facts: _Facts,
    itid: int,
    start: int,
    end: int,
    *,
    node_path: tuple[tuple[int, int], ...],
    before_node_ts: int | None,
    source_boundary: bool = False,
) -> list[dict[str, Any]]:
    metadata = facts.metadata(itid)
    states = facts.states(itid, start, end)
    active_path = node_path
    if before_node_ts is not None:
        tail = states[-1]
        if tail["end"] != end or (
            not source_boundary and tail["state"] not in _SELF_OWNED_STATES
        ):
            raise AttributionError(
                f"waker itid {itid} has no executing span ending at {end}"
            )
        node = (itid, tail["start"])
        if node[1] >= before_node_ts:
            raise AttributionError("wakeup graph does not advance to an earlier node")
        if node in node_path:
            raise AttributionError(f"wakeup graph contains a cycle at node {node}")
        active_path = (*node_path, node)
    attributed: list[dict[str, Any]] = []
    for state in states:
        if source_boundary or state["state"] in _SELF_OWNED_STATES:
            attributed.append({**state, "metadata": metadata})
            continue
        waker_itid = facts.waker(itid, state["start"], state["end"])
        if waker_itid == itid:
            attributed.append({**state, "metadata": metadata})
            continue
        waker = facts.metadata(waker_itid)
        name = waker["thread_name"]
        is_boundary = name == "udk-irq" or _OPENHARMONY_IO_WORKER_RULES.get(
            name, False
        )
        current_node = (itid, state["end"])
        descend_path = (
            active_path
            if active_path and active_path[-1] == current_node
            else (*active_path, current_node)
        )
        attributed.extend(
            _attribute(
                facts,
                waker_itid,
                state["start"],
                state["end"],
                node_path=descend_path,
                before_node_ts=state["end"],
                source_boundary=is_boundary,
            )
        )
    return attributed


def _complete_state_cover(
    rows: list[dict[str, Any]], itid: int, start: int, end: int
) -> list[dict[str, Any]]:
    clipped: list[dict[str, Any]] = []
    for row in rows:
        if _require_int(row, "itid", "thread state") != itid:
            raise AttributionError("thread state belongs to an unexpected thread")
        row_start = _require_int(row, "ts", "thread state")
        row_end = _checked_end(
            row_start,
            _require_int(row, "dur", "thread state", positive=True),
            "thread state",
        )
        state = row["thread_state_name"]
        if type(state) is not str or not state:
            raise AttributionError("thread state name must be non-empty")
        cpu = row["cpu"]
        io_wait = row["io_wait"]
        if cpu is not None and type(cpu) is not int:
            raise AttributionError("thread state cpu must be an integer or null")
        if io_wait is not None and (type(io_wait) is not int or io_wait not in {0, 1}):
            raise AttributionError("thread state io_wait must be zero, one, or null")
        if state == "Running" and cpu is None:
            raise AttributionError("Running thread state must identify its CPU")
        blocked_function = row["decoded_blocked_function"]
        if blocked_function is not None and type(blocked_function) is not str:
            raise AttributionError("thread state blocked_function must be a string or null")
        clipped_start = max(start, row_start)
        clipped_end = min(end, row_end)
        if clipped_start < clipped_end:
            clipped.append(
                {
                    "start": clipped_start,
                    "end": clipped_end,
                    "state": state,
                    "cpu": cpu,
                    "io_wait": io_wait,
                    "blocked_function": blocked_function,
                }
            )
    clipped.sort(key=lambda row: (row["start"], row["end"], row["state"]))
    cursor = start
    for row in clipped:
        if row["start"] != cursor:
            raise AttributionError(
                f"thread states for itid {itid} do not exactly cover [{start}, {end})"
            )
        cursor = row["end"]
    if cursor != end:
        raise AttributionError(
            f"thread states for itid {itid} do not exactly cover [{start}, {end})"
        )
    return clipped


def _validate_output_cover(rows: list[dict[str, Any]], start: int, end: int) -> None:
    rows.sort(key=lambda row: row["clock_value"])
    cursor = start
    for row in rows:
        if row["clock_value"] != cursor or row["duration_ns"] <= 0:
            raise AttributionError(
                f"scheduling attribution does not exactly cover [{start}, {end})"
            )
        cursor = _checked_end(row["clock_value"], row["duration_ns"], "output")
    if cursor != end:
        raise AttributionError(
            f"scheduling attribution does not exactly cover [{start}, {end})"
        )


def _rows(frame: Any, label: str, *, max_rows: int) -> list[dict[str, Any]]:
    try:
        batches = frame.collect()
    except Exception as error:
        raise AttributionError(f"failed to read {label}") from error
    row_count = sum(batch.num_rows for batch in batches)
    if row_count > max_rows:
        raise AttributionError(f"{label} exceeds the bounded Reader result")
    try:
        return pa.Table.from_batches(batches).to_pylist() if batches else []
    except Exception as error:
        raise AttributionError(f"failed to decode {label}") from error


def _require_int(
    row: dict[str, Any], name: str, label: str, *, positive: bool = False
) -> int:
    value = row[name]
    if type(value) is not int or (positive and value <= 0):
        qualifier = "positive " if positive else ""
        raise AttributionError(f"{label} {name} must be a {qualifier}integer")
    return value


def _checked_end(start: int, duration: int, label: str) -> int:
    end = start + duration
    if end <= start or end > 2**63 - 1:
        raise AttributionError(f"{label} interval is invalid")
    return end
