from __future__ import annotations

from collections.abc import Iterable
from typing import Any

from .model import CallstackSlice, StateSegment, ThreadRef, TraceFacts, WakeupEdge


ROOT_THREAD_SQL = """
SELECT
  t.id AS thread_row_id,
  t.itid,
  t.tid,
  t.name AS thread_name,
  p.pid,
  p.name AS process_name
FROM thread t
LEFT JOIN process p ON p.ipid = t.ipid
WHERE (:root_itid > 0 AND t.itid = :root_itid)
   OR (:root_tid > 0 AND t.tid = :root_tid AND (:root_pid <= 0 OR p.pid = :root_pid))
   OR (:root_thread_name_pattern <> '' AND regexp_match(t.name, :root_thread_name_pattern) IS NOT NULL)
ORDER BY
  CASE WHEN :root_itid > 0 AND t.itid = :root_itid THEN 0 ELSE 1 END,
  CASE WHEN :root_tid > 0 AND t.tid = :root_tid THEN 0 ELSE 1 END,
  t.itid ASC
LIMIT 1
"""

THREADS_SQL = """
SELECT
  t.id AS thread_row_id,
  t.itid,
  t.tid,
  t.name AS thread_name,
  p.pid,
  p.name AS process_name
FROM thread t
LEFT JOIN process p ON p.ipid = t.ipid
ORDER BY t.itid ASC
"""

STATE_SQL_FULL = """
SELECT
  id,
  itid,
  ts,
  dur,
  state,
  blocked_function,
  finnal_blocked_caller,
  io_wait
FROM thread_state
WHERE ts < :end_ts
  AND ts + dur > :start_ts
ORDER BY itid ASC, ts ASC, id ASC
"""

STATE_SQL_BASE = """
SELECT
  id,
  itid,
  ts,
  dur,
  state
FROM thread_state
WHERE ts < :end_ts
  AND ts + dur > :start_ts
ORDER BY itid ASC, ts ASC, id ASC
"""

WAKEUP_SQL = """
SELECT
  rowid AS id,
  ts,
  ref AS target_itid,
  wakeup_from AS waker_itid,
  name
FROM instant
WHERE name IN ('sched_wakeup', 'sched_wakeup_new', 'sched_waking')
  AND ref_type = 'itid'
  AND wakeup_from IS NOT NULL
  AND ts >= :start_ts
  AND ts <= :end_ts
ORDER BY ts ASC, rowid ASC
"""


def callstack_sql_for_itids(itids: Iterable[int]) -> str:
    values = sorted({int(itid) for itid in itids if int(itid) > 0})
    if not values:
        values = [-1]
    literal_list = ", ".join(str(value) for value in values)
    return f"""
SELECT
  id,
  callid AS itid,
  ts,
  dur,
  name,
  depth,
  parent_id
FROM callstack
WHERE callid IN ({literal_list})
  AND ts < :end_ts
  AND ts + dur > :start_ts
ORDER BY callid ASC, ts ASC, depth ASC, id ASC
"""


def bounded_rows(query_result: Any, max_rows: int) -> list[dict[str, Any]]:
    if max_rows <= 0:
        raise ValueError("max_rows must be positive")
    rows: Any
    if isinstance(query_result, list):
        rows = query_result
    elif isinstance(query_result, dict) and "rows" in query_result:
        rows = query_result["rows"]
    else:
        rows = _call_row_method(query_result, max_rows)
    normalized = [_row_to_dict(row) for row in rows]
    if len(normalized) > max_rows:
        raise RuntimeError(f"bounded query returned more than {max_rows} rows")
    return normalized


def query_rows(kat: Any, sql: str, max_rows: int, **params: Any) -> list[dict[str, Any]]:
    return bounded_rows(kat.query(sql, **params), max_rows=max_rows)


def query_window_facts(
    kat: Any,
    root_itid: int,
    root_tid: int,
    root_pid: int,
    root_thread_name_pattern: str,
    start_ts: int,
    end_ts: int,
    max_fact_rows: int,
) -> tuple[int, TraceFacts]:
    root_rows = query_rows(
        kat,
        ROOT_THREAD_SQL,
        max_rows=1,
        root_itid=root_itid,
        root_tid=root_tid,
        root_pid=root_pid,
        root_thread_name_pattern=root_thread_name_pattern,
    )
    if not root_rows:
        raise ValueError("root thread could not be resolved")
    resolved_root_itid = int(root_rows[0]["itid"])

    thread_rows = query_rows(kat, THREADS_SQL, max_rows=max_fact_rows)
    state_rows = _query_state_rows(kat, start_ts, end_ts, max_fact_rows)
    wakeup_rows = query_rows(kat, WAKEUP_SQL, max_rows=max_fact_rows, start_ts=start_ts, end_ts=end_ts)
    seed_itids = {resolved_root_itid}
    seed_itids.update(_as_int(row.get("itid")) for row in state_rows)
    seed_itids.update(_as_int(row.get("waker_itid")) for row in wakeup_rows)
    callstack_rows = _query_callstacks(kat, seed_itids, start_ts, end_ts, max_fact_rows)

    facts = TraceFacts(
        threads=_threads(thread_rows),
        states=_states(state_rows),
        wakeups=_wakeups(wakeup_rows),
        callstacks=_callstacks(callstack_rows),
    )
    return resolved_root_itid, facts


def _query_state_rows(kat: Any, start_ts: int, end_ts: int, max_fact_rows: int) -> list[dict[str, Any]]:
    try:
        return query_rows(kat, STATE_SQL_FULL, max_rows=max_fact_rows, start_ts=start_ts, end_ts=end_ts)
    except Exception:
        rows = query_rows(kat, STATE_SQL_BASE, max_rows=max_fact_rows, start_ts=start_ts, end_ts=end_ts)
        for row in rows:
            row.setdefault("blocked_function", "")
            row.setdefault("finnal_blocked_caller", "")
            row.setdefault("io_wait", False)
        return rows


def _query_callstacks(
    kat: Any,
    itids: set[int],
    start_ts: int,
    end_ts: int,
    max_fact_rows: int,
) -> list[dict[str, Any]]:
    try:
        return query_rows(
            kat,
            callstack_sql_for_itids(itids),
            max_rows=max_fact_rows,
            start_ts=start_ts,
            end_ts=end_ts,
        )
    except Exception:
        return []


def _call_row_method(query_result: Any, max_rows: int) -> Any:
    for name in ("rows", "collect", "to_rows", "preview"):
        method = getattr(query_result, name, None)
        if method is None:
            continue
        try:
            return method(max_rows=max_rows)
        except TypeError:
            return method(limit=max_rows)
    raise RuntimeError("QueryResult does not expose a bounded row method")


def _row_to_dict(row: Any) -> dict[str, Any]:
    if isinstance(row, dict):
        return dict(row)
    if hasattr(row, "_asdict"):
        return dict(row._asdict())
    if hasattr(row, "__dict__"):
        return dict(row.__dict__)
    raise TypeError(f"cannot convert row to dict: {row!r}")


def _threads(rows: list[dict[str, Any]]) -> dict[int, ThreadRef]:
    result: dict[int, ThreadRef] = {}
    for row in rows:
        itid = _as_int(row.get("itid"))
        if itid <= 0:
            continue
        result[itid] = ThreadRef(
            itid=itid,
            tid=_optional_int(row.get("tid")),
            name=str(row.get("thread_name") or ""),
            pid=_optional_int(row.get("pid")),
            process_name=str(row.get("process_name") or ""),
        )
    return result


def _states(rows: list[dict[str, Any]]) -> list[StateSegment]:
    return [
        StateSegment(
            id=_optional_int(row.get("id")),
            itid=_as_int(row.get("itid")),
            ts=_as_int(row.get("ts")),
            dur=max(0, _as_int(row.get("dur"))),
            state=str(row.get("state") or ""),
            blocked_function=str(row.get("blocked_function") or ""),
            final_blocked_caller=str(row.get("finnal_blocked_caller") or ""),
            io_wait=_as_bool(row.get("io_wait")),
        )
        for row in rows
        if _as_int(row.get("itid")) > 0
    ]


def _wakeups(rows: list[dict[str, Any]]) -> list[WakeupEdge]:
    return [
        WakeupEdge(
            id=_optional_int(row.get("id")),
            ts=_as_int(row.get("ts")),
            target_itid=_as_int(row.get("target_itid")),
            waker_itid=_optional_int(row.get("waker_itid")),
            name=str(row.get("name") or "sched_wakeup"),
        )
        for row in rows
        if _as_int(row.get("target_itid")) > 0
    ]


def _callstacks(rows: list[dict[str, Any]]) -> list[CallstackSlice]:
    return [
        CallstackSlice(
            id=_optional_int(row.get("id")),
            itid=_as_int(row.get("itid")),
            ts=_as_int(row.get("ts")),
            dur=max(0, _as_int(row.get("dur"))),
            name=str(row.get("name") or ""),
            depth=_optional_int(row.get("depth")),
            parent_id=_optional_int(row.get("parent_id")),
        )
        for row in rows
        if _as_int(row.get("itid")) > 0
    ]


def _as_int(value: Any) -> int:
    if value is None or value == "":
        return 0
    return int(value)


def _optional_int(value: Any) -> int | None:
    if value is None or value == "":
        return None
    return int(value)


def _as_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    if isinstance(value, (int, float)):
        return value != 0
    return str(value).strip().lower() in {"1", "true", "yes", "y"}
