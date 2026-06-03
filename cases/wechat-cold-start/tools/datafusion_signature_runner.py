#!/usr/bin/env python3
"""
Run a deterministic Harmony cold-start signature against a DataFusion-backed
kat-rs Web UI dataset.

The script deliberately keeps the transport small: it only uploads/chooses a
dataset and sends SQL to /api/query. All evidence comes from DataFusion tables.
"""

from __future__ import annotations

import argparse
import datetime as dt
import http.client
import json
import os
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_SERVER = "http://127.0.0.1:8787"
DEFAULT_TARGET_PACKAGE = "com.tencent.wechat"
DEFAULT_TARGET_PROCESS = ".tencent.wechat"
DEFAULT_FALLBACK_MARKER = "IconStart com.tencent.wechat"
HOTSPOT_KEYWORDS = [
    "JsRuntime::LoadModule",
    "JsRuntime::RunScript",
    "EntryAbility.abc",
    "SourceTextModule::Evaluate",
]
DEFAULT_OUT_DIR = str(Path(__file__).resolve().parents[1] / "signature-output" / "latest")


def sql_string(value: str | None) -> str:
    if value is None:
        return "''"
    return "'" + value.replace("'", "''") + "'"


def like_pattern(value: str | None) -> str:
    if not value:
        return "%%"
    return "%" + value.replace("'", "''") + "%"


def parse_cpu_csv(value: str) -> list[int]:
    cpus: list[int] = []
    for part in value.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            start, end = part.split("-", 1)
            cpus.extend(range(int(start), int(end) + 1))
        else:
            cpus.append(int(part))
    return sorted(set(cpus))


class KatRsClient:
    def __init__(self, server: str):
        self.server = server.rstrip("/")

    def _url(self, path: str) -> str:
        return f"{self.server}{path}"

    def get_json(self, path: str) -> dict[str, Any]:
        with urllib.request.urlopen(self._url(path), timeout=120) as response:
            return json.loads(response.read().decode("utf-8"))

    def post_json(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            self._url(path),
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=300) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"HTTP {exc.code}: {body}") from exc

    def upload_trace(self, trace_path: Path) -> str:
        boundary = "----kat-rs-signature-boundary"
        filename = trace_path.name
        header = (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="trace"; filename="{filename}"\r\n'
            "Content-Type: application/octet-stream\r\n\r\n"
        ).encode("utf-8")
        footer = f"\r\n--{boundary}--\r\n".encode("utf-8")
        size = trace_path.stat().st_size

        parsed = urllib.parse.urlparse(self.server)
        if parsed.scheme != "http":
            raise RuntimeError("Streaming upload currently supports http:// servers only")
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 80
        path = "/api/datasets/upload"

        conn = http.client.HTTPConnection(host, port, timeout=600)
        try:
            conn.putrequest("POST", path)
            conn.putheader("Content-Type", f"multipart/form-data; boundary={boundary}")
            conn.putheader("Content-Length", str(len(header) + size + len(footer)))
            conn.endheaders()
            conn.send(header)
            with trace_path.open("rb") as handle:
                while True:
                    chunk = handle.read(1024 * 1024)
                    if not chunk:
                        break
                    conn.send(chunk)
            conn.send(footer)
            response = conn.getresponse()
            body = response.read().decode("utf-8", errors="replace")
            if response.status >= 400:
                raise RuntimeError(f"upload failed HTTP {response.status}: {body}")
            payload = json.loads(body)
            return payload["dataset_id"]
        finally:
            conn.close()

    def active_dataset_id(self) -> str:
        payload = self.get_json("/api/datasets")
        dataset_id = payload.get("active_dataset_id")
        if dataset_id:
            return dataset_id
        datasets = payload.get("datasets") or []
        if not datasets:
            raise RuntimeError("no active DataFusion dataset")
        return datasets[0]["dataset_id"]

    def query(self, dataset_id: str, sql: str, max_rows: int = 1000) -> list[dict[str, Any]]:
        payload = self.post_json(
            "/api/query",
            {
                "dataset_id": dataset_id,
                "sql": sql,
                "max_inline_rows": max_rows,
            },
        )
        return payload.get("rows") or []

    def inspect(self, dataset_id: str) -> dict[str, Any]:
        encoded = urllib.parse.quote(dataset_id, safe="")
        return self.get_json(f"/api/inspect?dataset_id={encoded}")


def choose_process(rows: list[dict[str, Any]], target_process: str) -> dict[str, Any] | None:
    exact = [r for r in rows if str(r.get("process_name") or "").lower() == target_process.lower()]
    candidates = exact or rows
    candidates = sorted(
        candidates,
        key=lambda r: (
            r.get("start_ts") is None,
            r.get("start_ts") or 0,
            r.get("pid") or 0,
        ),
    )
    return candidates[0] if candidates else None


def phase_rows(anchors: dict[str, Any]) -> list[dict[str, Any]]:
    spans = [
        (
            1,
            "A_input_dispatch_to_app",
            anchors["start_anchor_name"],
            "HandleLaunchApplication",
            anchors["t_start"],
            anchors["t_launch_application"],
        ),
        (
            2,
            "B_launch_application_to_ability",
            "HandleLaunchApplication",
            "HandleLaunchAbility",
            anchors["t_launch_application"],
            anchors["t_launch_ability"],
        ),
        (
            3,
            "C_launch_ability_to_transaction",
            "HandleLaunchAbility",
            "HandleAbilityTransaction",
            anchors["t_launch_ability"],
            anchors["t_ability_transaction"],
        ),
        (
            4,
            "D_transaction_to_vsync",
            "HandleAbilityTransaction",
            "OnVsyncEvent now",
            anchors["t_ability_transaction"],
            anchors["t_first_vsync"],
        ),
        (
            99,
            "TOTAL_start_to_vsync",
            anchors["start_anchor_name"],
            "OnVsyncEvent now",
            anchors["t_start"],
            anchors["t_first_vsync"],
        ),
    ]
    rows: list[dict[str, Any]] = []
    for order, phase, start_anchor, end_anchor, start_ts, end_ts in spans:
        elapsed_ns = end_ts - start_ts
        rows.append(
            {
                "phase_order": order,
                "phase": phase,
                "start_anchor": start_anchor,
                "end_anchor": end_anchor,
                "start_ts": start_ts,
                "end_ts": end_ts,
                "elapsed_ns": elapsed_ns,
                "elapsed_ms": round(elapsed_ns / 1_000_000.0, 3),
            }
        )
    return rows


def max_phase(phases: list[dict[str, Any]]) -> dict[str, Any]:
    return max((p for p in phases if p["phase_order"] != 99), key=lambda p: p["elapsed_ns"])


def running_ratio_for_phase(states: list[dict[str, Any]], phase: str, phase_ms: float) -> float:
    running = sum(r.get("duration_ms") or 0.0 for r in states if r["phase"] == phase and r["state"] == "running")
    if phase_ms <= 0:
        return 0.0
    return running / phase_ms


def small_ratio(cluster_totals: list[dict[str, Any]]) -> float:
    total = sum(r.get("running_ms") or 0.0 for r in cluster_totals)
    small = sum(r.get("running_ms") or 0.0 for r in cluster_totals if r.get("cluster_name") == "small")
    return small / total if total > 0 else 0.0


def first_row(rows: list[dict[str, Any]], predicate) -> dict[str, Any] | None:
    for row in rows:
        if predicate(row):
            return row
    return None


def build_cpu_cluster_sql(small: list[int], middle: list[int], big: list[int]) -> str:
    parts: list[str] = []
    for cpu in small:
        parts.append(f"SELECT {cpu} AS cpu, 'small' AS cluster_name")
    for cpu in middle:
        parts.append(f"SELECT {cpu} AS cpu, 'middle' AS cluster_name")
    for cpu in big:
        parts.append(f"SELECT {cpu} AS cpu, 'big' AS cluster_name")
    return "\n  UNION ALL ".join(parts)


def run_signature(args: argparse.Namespace) -> dict[str, Any]:
    client = KatRsClient(args.server)
    if args.trace:
        dataset_id = client.upload_trace(Path(args.trace))
    else:
        dataset_id = args.dataset_id or client.active_dataset_id()

    inspect = client.inspect(dataset_id)
    trace = inspect.get("trace", {})
    tables = inspect.get("tables", {})

    target_process_lit = sql_string(args.target_process)
    target_package_like = like_pattern(args.target_package)
    target_process_like = like_pattern(args.target_process)

    process_sql = f"""
SELECT
  p.upid,
  p.pid,
  p.name AS process_name,
  p.start_ts,
  p.end_ts,
  t.utid AS main_utid,
  t.tid AS main_tid,
  t.name AS main_thread_name,
  CASE
    WHEN lower(coalesce(p.name, '')) = lower({target_process_lit}) THEN 'exact_process'
    WHEN lower(coalesce(p.name, '')) LIKE lower('{target_package_like}') THEN 'package_like'
    WHEN lower(coalesce(p.name, '')) LIKE lower('{target_process_like}') THEN 'process_like'
    WHEN lower(coalesce(t.name, '')) LIKE lower('{target_package_like}') THEN 'thread_like'
    ELSE 'weak_keyword'
  END AS match_reason,
  CASE
    WHEN lower(coalesce(p.name, '')) = lower({target_process_lit}) THEN 'high'
    WHEN lower(coalesce(p.name, '')) LIKE lower('{target_package_like}') THEN 'medium'
    ELSE 'low'
  END AS confidence
FROM process p
LEFT JOIN thread t ON t.upid = p.upid AND t.is_main = true
WHERE lower(coalesce(p.name, '')) = lower({target_process_lit})
   OR lower(coalesce(p.name, '')) LIKE lower('{target_package_like}')
   OR lower(coalesce(p.name, '')) LIKE lower('{target_process_like}')
   OR lower(coalesce(t.name, '')) LIKE lower('{target_package_like}')
   OR lower(coalesce(t.name, '')) LIKE lower('{target_process_like}')
ORDER BY p.start_ts, p.pid
"""
    process_rows = client.query(dataset_id, process_sql, 500)
    selected_process = choose_process(process_rows, args.target_process)
    if not selected_process:
        return {
            "trace": trace,
            "dataset_id": dataset_id,
            "status": "inconclusive",
            "reason": "target process not found",
            "process_candidates": process_rows,
        }

    upid = int(selected_process["upid"])
    main_utid = int(selected_process["main_utid"])
    main_tid = int(selected_process["main_tid"])

    tags_sql = f"""
WITH tag_events AS (
  SELECT
    ts,
    cpu,
    tid,
    event_name,
    payload_json,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%' OR coalesce(payload_json, '') LIKE '%touchEventDispatch%' THEN 'touchEventDispatch'
      WHEN event_name LIKE '%HandleLaunchApplication%' OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%' THEN 'HandleLaunchApplication'
      WHEN event_name LIKE '%HandleLaunchAbility%' OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%' THEN 'HandleLaunchAbility'
      WHEN event_name LIKE '%HandleAbilityTransaction%' OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%' THEN 'HandleAbilityTransaction'
      WHEN event_name LIKE '%OnVsyncEvent now%' OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%' THEN 'OnVsyncEvent now'
      ELSE 'unknown'
    END AS tag_name,
    CASE
      WHEN event_name LIKE '%touchEventDispatch%' OR coalesce(payload_json, '') LIKE '%touchEventDispatch%' THEN 1
      WHEN event_name LIKE '%HandleLaunchApplication%' OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%' THEN 2
      WHEN event_name LIKE '%HandleLaunchAbility%' OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%' THEN 3
      WHEN event_name LIKE '%HandleAbilityTransaction%' OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%' THEN 4
      WHEN event_name LIKE '%OnVsyncEvent now%' OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%' THEN 5
      ELSE 99
    END AS tag_order
  FROM raw_event
  WHERE event_name LIKE '%touchEventDispatch%'
     OR event_name LIKE '%HandleLaunchApplication%'
     OR event_name LIKE '%HandleLaunchAbility%'
     OR event_name LIKE '%HandleAbilityTransaction%'
     OR event_name LIKE '%OnVsyncEvent now%'
     OR coalesce(payload_json, '') LIKE '%touchEventDispatch%'
     OR coalesce(payload_json, '') LIKE '%HandleLaunchApplication%'
     OR coalesce(payload_json, '') LIKE '%HandleLaunchAbility%'
     OR coalesce(payload_json, '') LIKE '%HandleAbilityTransaction%'
     OR coalesce(payload_json, '') LIKE '%OnVsyncEvent now%'
),
tag_with_process AS (
  SELECT
    e.tag_order,
    e.tag_name,
    e.ts,
    e.cpu,
    e.tid,
    t.utid,
    t.upid,
    p.pid,
    p.name AS process_name,
    t.name AS thread_name,
    t.is_main,
    CASE
      WHEN e.tag_name = 'touchEventDispatch' THEN 'input_side'
      WHEN t.upid = {upid} THEN 'target_process'
      ELSE 'non_target'
    END AS process_role,
    e.event_name,
    e.payload_json
  FROM tag_events e
  LEFT JOIN thread t ON e.tid = t.tid
  LEFT JOIN process p ON t.upid = p.upid
)
SELECT *
FROM tag_with_process
ORDER BY ts, tag_order
"""
    tag_rows = client.query(dataset_id, tags_sql, 5000)

    def target_tag(name: str, after: int | None = None) -> dict[str, Any] | None:
        return first_row(
            tag_rows,
            lambda r: r["tag_name"] == name
            and r["process_role"] == "target_process"
            and (after is None or int(r["ts"]) >= after),
        )

    launch_application = target_tag("HandleLaunchApplication")
    launch_ability = target_tag("HandleLaunchAbility", int(launch_application["ts"]) if launch_application else None)
    ability_transaction = target_tag("HandleAbilityTransaction", int(launch_ability["ts"]) if launch_ability else None)
    first_vsync = target_tag("OnVsyncEvent now", int(ability_transaction["ts"]) if ability_transaction else None)
    if not (launch_application and launch_ability and ability_transaction and first_vsync):
        return {
            "trace": trace,
            "dataset_id": dataset_id,
            "status": "inconclusive",
            "reason": "required target-process cold-start tags not found",
            "selected_process": selected_process,
            "tag_candidates": tag_rows,
        }

    launch_ts = int(launch_application["ts"])
    touch_candidates = [
        r for r in tag_rows if r["tag_name"] == "touchEventDispatch" and r.get("ts") is not None and int(r["ts"]) <= launch_ts
    ]
    start_anchor = None
    anchor_confidence = "high"
    if touch_candidates:
        start_anchor = sorted(touch_candidates, key=lambda r: int(r["ts"]))[-1]
        start_ts = int(start_anchor["ts"])
        start_anchor_name = "touchEventDispatch"
    else:
        marker = args.fallback_marker.replace("'", "''")
        fallback_sql = f"""
SELECT
  r.ts,
  r.cpu,
  r.tid,
  t.utid,
  t.upid,
  p.pid,
  p.name AS process_name,
  t.name AS thread_name,
  r.event_name,
  r.payload_json
FROM raw_event r
LEFT JOIN thread t ON r.tid = t.tid
LEFT JOIN process p ON t.upid = p.upid
WHERE (r.event_name LIKE '%{marker}%' OR coalesce(r.payload_json, '') LIKE '%{marker}%')
  AND r.ts <= {launch_ts}
ORDER BY r.ts DESC
LIMIT 1
"""
        fallback_rows = client.query(dataset_id, fallback_sql, 10)
        if not fallback_rows:
            return {
                "trace": trace,
                "dataset_id": dataset_id,
                "status": "inconclusive",
                "reason": "missing touchEventDispatch and fallback marker",
                "selected_process": selected_process,
                "tag_candidates": tag_rows,
            }
        start_anchor = fallback_rows[0]
        start_ts = int(start_anchor["ts"])
        start_anchor_name = args.fallback_marker
        anchor_confidence = "fallback"

    anchors = {
        "start_anchor_name": start_anchor_name,
        "anchor_confidence": anchor_confidence,
        "t_start": start_ts,
        "t_launch_application": int(launch_application["ts"]),
        "t_launch_ability": int(launch_ability["ts"]),
        "t_ability_transaction": int(ability_transaction["ts"]),
        "t_first_vsync": int(first_vsync["ts"]),
        "start_anchor": start_anchor,
        "launch_application": launch_application,
        "launch_ability": launch_ability,
        "ability_transaction": ability_transaction,
        "first_vsync": first_vsync,
    }

    phases = phase_rows(anchors)
    largest_phase = max_phase(phases)
    total_phase = next(p for p in phases if p["phase_order"] == 99)
    max_ratio = largest_phase["elapsed_ns"] / total_phase["elapsed_ns"] if total_phase["elapsed_ns"] > 0 else 0.0

    states_sql = f"""
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, {anchors["t_start"]} AS start_ts, {anchors["t_launch_application"]} AS end_ts
  UNION ALL SELECT 'B_launch_application_to_ability', {anchors["t_launch_application"]}, {anchors["t_launch_ability"]}
  UNION ALL SELECT 'C_launch_ability_to_transaction', {anchors["t_launch_ability"]}, {anchors["t_ability_transaction"]}
  UNION ALL SELECT 'D_transaction_to_vsync', {anchors["t_ability_transaction"]}, {anchors["t_first_vsync"]}
),
overlap AS (
  SELECT
    p.phase,
    st.state,
    st.io_wait,
    st.blocked_function,
    st.waker_utid,
    CASE WHEN st.ts > p.start_ts THEN st.ts ELSE p.start_ts END AS overlap_start,
    CASE
      WHEN st.ts + coalesce(st.dur, p.end_ts - st.ts) < p.end_ts
      THEN st.ts + coalesce(st.dur, p.end_ts - st.ts)
      ELSE p.end_ts
    END AS overlap_end
  FROM phase_span p
  JOIN thread_state st
    ON st.utid = {main_utid}
   AND st.ts < p.end_ts
   AND st.ts + coalesce(st.dur, p.end_ts - st.ts) > p.start_ts
)
SELECT
  phase,
  state,
  io_wait,
  blocked_function,
  waker_utid,
  SUM(overlap_end - overlap_start) AS duration_ns,
  ROUND(SUM(overlap_end - overlap_start) / 1000000.0, 3) AS duration_ms,
  COUNT(*) AS sample_count
FROM overlap
WHERE overlap_end > overlap_start
GROUP BY phase, state, io_wait, blocked_function, waker_utid
ORDER BY phase, duration_ns DESC
"""
    states = client.query(dataset_id, states_sql, 500)

    c_start = int(largest_phase["start_ts"])
    c_end = int(largest_phase["end_ts"])
    range_sql = f"""
WITH target_threads AS (
  SELECT p.upid, p.pid, p.name AS process_name, t.utid, t.tid, t.name AS thread_name, t.is_main
  FROM process p
  JOIN thread t ON t.upid = p.upid
  WHERE p.upid = {upid}
),
callstack_overlap AS (
  SELECT
    'callstack' AS source,
    'running_span' AS path_kind,
    CASE WHEN cs.ts > {c_start} THEN cs.ts ELSE {c_start} END AS ts,
    CASE
      WHEN cs.ts + coalesce(cs.dur, {c_end} - cs.ts) < {c_end}
      THEN cs.ts + coalesce(cs.dur, {c_end} - cs.ts)
      ELSE {c_end}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    cs.name AS span_name,
    CAST(NULL AS VARCHAR) AS state,
    CAST(NULL AS BOOLEAN) AS io_wait,
    CAST(NULL AS VARCHAR) AS blocked_function,
    CAST(NULL AS BIGINT) AS waker_utid,
    CAST(NULL AS BIGINT) AS cpu,
    CAST(cs.depth AS BIGINT) AS depth,
    CAST(cs.parent_id AS BIGINT) AS parent_id,
    'long callstack span overlaps target range' AS reason
  FROM callstack cs
  JOIN target_threads tt ON cs.callid = tt.utid OR cs.callid = tt.tid
  WHERE cs.ts < {c_end}
    AND cs.ts + coalesce(cs.dur, {c_end} - cs.ts) > {c_start}
    AND coalesce(cs.dur, 0) >= {args.min_span_ms} * 1000000
),
state_overlap AS (
  SELECT
    'thread_state' AS source,
    CASE
      WHEN st.state = 'running' THEN 'running_state'
      WHEN st.state = 'runnable' THEN 'runnable_wait'
      WHEN st.io_wait = true THEN 'io_wait'
      WHEN st.state = 'uninterruptible' THEN 'blocking_wait'
      ELSE 'sleeping'
    END AS path_kind,
    CASE WHEN st.ts > {c_start} THEN st.ts ELSE {c_start} END AS ts,
    CASE
      WHEN st.ts + coalesce(st.dur, {c_end} - st.ts) < {c_end}
      THEN st.ts + coalesce(st.dur, {c_end} - st.ts)
      ELSE {c_end}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    CAST(NULL AS VARCHAR) AS span_name,
    st.state,
    st.io_wait,
    st.blocked_function,
    CAST(st.waker_utid AS BIGINT) AS waker_utid,
    CAST(NULL AS BIGINT) AS cpu,
    CAST(NULL AS BIGINT) AS depth,
    CAST(NULL AS BIGINT) AS parent_id,
    'thread state overlaps target range' AS reason
  FROM thread_state st
  JOIN target_threads tt ON st.utid = tt.utid
  WHERE st.ts < {c_end}
    AND st.ts + coalesce(st.dur, {c_end} - st.ts) > {c_start}
    AND coalesce(st.dur, 0) >= {args.min_span_ms} * 1000000
),
sched_overlap AS (
  SELECT
    'sched_slice' AS source,
    'cpu_running' AS path_kind,
    CASE WHEN s.ts > {c_start} THEN s.ts ELSE {c_start} END AS ts,
    CASE
      WHEN s.ts + coalesce(s.dur, {c_end} - s.ts) < {c_end}
      THEN s.ts + coalesce(s.dur, {c_end} - s.ts)
      ELSE {c_end}
    END AS end_ts,
    tt.upid,
    tt.pid,
    tt.process_name,
    tt.utid,
    tt.tid,
    tt.thread_name,
    tt.is_main,
    CAST(NULL AS VARCHAR) AS span_name,
    CAST(NULL AS VARCHAR) AS state,
    CAST(NULL AS BOOLEAN) AS io_wait,
    CAST(NULL AS VARCHAR) AS blocked_function,
    CAST(NULL AS BIGINT) AS waker_utid,
    CAST(s.cpu AS BIGINT) AS cpu,
    CAST(NULL AS BIGINT) AS depth,
    CAST(NULL AS BIGINT) AS parent_id,
    'actual CPU running slice overlaps target range' AS reason
  FROM sched_slice s
  JOIN target_threads tt ON s.utid = tt.utid
  WHERE s.ts < {c_end}
    AND s.ts + coalesce(s.dur, {c_end} - s.ts) > {c_start}
    AND coalesce(s.dur, 0) >= {args.min_span_ms} * 1000000
),
merged AS (
  SELECT * FROM callstack_overlap
  UNION ALL SELECT * FROM state_overlap
  UNION ALL SELECT * FROM sched_overlap
),
ranked AS (
  SELECT
    ROW_NUMBER() OVER (ORDER BY end_ts - ts DESC, ts ASC) AS path_rank,
    source,
    path_kind,
    ts,
    end_ts,
    end_ts - ts AS dur_ns,
    ROUND((end_ts - ts) / 1000000.0, 3) AS dur_ms,
    upid,
    pid,
    process_name,
    utid,
    tid,
    thread_name,
    is_main,
    span_name,
    state,
    io_wait,
    blocked_function,
    waker_utid,
    cpu,
    depth,
    parent_id,
    reason
  FROM merged
  WHERE end_ts > ts
)
SELECT *
FROM ranked
ORDER BY path_rank
LIMIT {args.max_hotspots}
"""
    critical_path = client.query(dataset_id, range_sql, args.max_hotspots)

    cpu_cluster_sql = build_cpu_cluster_sql(
        parse_cpu_csv(args.small_cpus),
        parse_cpu_csv(args.middle_cpus),
        parse_cpu_csv(args.big_cpus),
    )
    cluster_sql = f"""
WITH phase_span AS (
  SELECT 'A_input_dispatch_to_app' AS phase, {anchors["t_start"]} AS start_ts, {anchors["t_launch_application"]} AS end_ts
  UNION ALL SELECT 'B_launch_application_to_ability', {anchors["t_launch_application"]}, {anchors["t_launch_ability"]}
  UNION ALL SELECT 'C_launch_ability_to_transaction', {anchors["t_launch_ability"]}, {anchors["t_ability_transaction"]}
  UNION ALL SELECT 'D_transaction_to_vsync', {anchors["t_ability_transaction"]}, {anchors["t_first_vsync"]}
),
cpu_cluster AS (
  {cpu_cluster_sql}
),
overlap AS (
  SELECT
    p.phase,
    coalesce(c.cluster_name, 'unknown') AS cluster_name,
    s.cpu,
    CASE WHEN s.ts > p.start_ts THEN s.ts ELSE p.start_ts END AS overlap_start,
    CASE
      WHEN s.ts + coalesce(s.dur, p.end_ts - s.ts) < p.end_ts
      THEN s.ts + coalesce(s.dur, p.end_ts - s.ts)
      ELSE p.end_ts
    END AS overlap_end
  FROM phase_span p
  JOIN sched_slice s
    ON s.utid = {main_utid}
   AND s.ts < p.end_ts
   AND s.ts + coalesce(s.dur, p.end_ts - s.ts) > p.start_ts
  LEFT JOIN cpu_cluster c ON c.cpu = s.cpu
)
SELECT
  phase,
  cluster_name,
  SUM(overlap_end - overlap_start) AS running_ns,
  ROUND(SUM(overlap_end - overlap_start) / 1000000.0, 3) AS running_ms,
  COUNT(*) AS slice_count,
  MIN(cpu) AS min_cpu,
  MAX(cpu) AS max_cpu
FROM overlap
WHERE overlap_end > overlap_start
GROUP BY phase, cluster_name
ORDER BY phase, cluster_name
"""
    cluster_by_phase = client.query(dataset_id, cluster_sql, 200)
    cluster_totals: dict[str, float] = {}
    for row in cluster_by_phase:
        cluster_totals[row["cluster_name"]] = cluster_totals.get(row["cluster_name"], 0.0) + (row.get("running_ms") or 0.0)
    cluster_total_rows = [
        {"cluster_name": name, "running_ms": round(value, 3)}
        for name, value in sorted(cluster_totals.items())
    ]

    c_running_ratio = running_ratio_for_phase(states, largest_phase["phase"], largest_phase["elapsed_ms"])
    s_ratio = small_ratio(cluster_total_rows)
    hotspot_match = any(
        any(keyword in str(row.get("span_name") or "") for keyword in HOTSPOT_KEYWORDS)
        for row in critical_path
    )
    predicates = {
        "max_phase_is_launch_ability": largest_phase["phase"] == "C_launch_ability_to_transaction",
        "max_phase_ratio_high": max_ratio >= args.max_phase_ratio_threshold,
        "main_thread_running_dominant": c_running_ratio >= args.running_ratio_threshold,
        "js_load_hotspot": hotspot_match,
        "not_small_core_issue": s_ratio < args.small_ratio_threshold,
    }
    status = "match" if all(predicates.values()) else "no_match"
    top_hotspot = next((r for r in critical_path if r.get("source") == "callstack"), None)

    return {
        "signature_id": "harmony_wechat_cold_start_js_load",
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "dataset_id": dataset_id,
        "trace": trace,
        "status": status,
        "selected_process": selected_process,
        "anchors": anchors,
        "phases": phases,
        "max_phase": largest_phase,
        "max_phase_ratio": round(max_ratio, 6),
        "main_running_ratio_in_max_phase": round(c_running_ratio, 6),
        "critical_path": critical_path,
        "thread_states": states,
        "cluster_by_phase": cluster_by_phase,
        "cluster_total": cluster_total_rows,
        "small_ratio": round(s_ratio, 6),
        "top_hotspot": top_hotspot,
        "predicates": predicates,
        "table_rows": {name: table.get("rows") for name, table in tables.items()},
    }


def write_markdown(result: dict[str, Any], path: Path) -> None:
    top = result.get("top_hotspot") or {}
    max_phase_data = result.get("max_phase") or {}
    selected = result.get("selected_process") or {}
    anchors = result.get("anchors") or {}
    cluster_total = result.get("cluster_total") or []
    lines = [
        f"# Signature Result: {result.get('signature_id')}",
        "",
        f"Status: `{result.get('status')}`",
        "",
        f"Trace: `{result.get('trace', {}).get('path') or result.get('trace', {}).get('trace_id')}`",
        "",
        "## Summary",
        "",
        f"- Target process: `{selected.get('process_name')}` pid={selected.get('pid')} upid={selected.get('upid')}",
        f"- Anchor confidence: `{anchors.get('anchor_confidence')}`",
        f"- Max phase: `{max_phase_data.get('phase')}` {max_phase_data.get('elapsed_ms')} ms",
        f"- Max phase ratio: `{result.get('max_phase_ratio')}`",
        f"- Main running ratio in max phase: `{result.get('main_running_ratio_in_max_phase')}`",
        f"- Small ratio: `{result.get('small_ratio')}`",
        f"- Top hotspot: `{top.get('span_name')}` {top.get('dur_ms')} ms",
        "",
        "## Predicates",
        "",
    ]
    for key, passed in (result.get("predicates") or {}).items():
        lines.append(f"- `{key}`: `{passed}`")
    lines += ["", "## CPU Cluster Total", ""]
    for row in cluster_total:
        lines.append(f"- `{row.get('cluster_name')}`: {row.get('running_ms')} ms")
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run DataFusion cold-start signature queries")
    parser.add_argument("--server", default=DEFAULT_SERVER)
    parser.add_argument("--dataset-id")
    parser.add_argument("--trace", help="Optional trace path to upload before querying")
    parser.add_argument("--target-package", default=DEFAULT_TARGET_PACKAGE)
    parser.add_argument("--target-process", default=DEFAULT_TARGET_PROCESS)
    parser.add_argument("--fallback-marker", default=DEFAULT_FALLBACK_MARKER)
    parser.add_argument("--small-cpus", default="0-3")
    parser.add_argument("--middle-cpus", default="4-9")
    parser.add_argument("--big-cpus", default="10-11")
    parser.add_argument("--min-span-ms", type=float, default=1.0)
    parser.add_argument("--max-hotspots", type=int, default=120)
    parser.add_argument("--max-phase-ratio-threshold", type=float, default=0.40)
    parser.add_argument("--running-ratio-threshold", type=float, default=0.70)
    parser.add_argument("--small-ratio-threshold", type=float, default=0.05)
    parser.add_argument("--out-dir", default=DEFAULT_OUT_DIR)
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    result = run_signature(args)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    json_path = out_dir / "signature_result.json"
    md_path = out_dir / "signature_result.md"
    json_path.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    write_markdown(result, md_path)
    print(json.dumps(
        {
            "status": result.get("status"),
            "json": str(json_path),
            "markdown": str(md_path),
            "max_phase": (result.get("max_phase") or {}).get("phase"),
            "small_ratio": result.get("small_ratio"),
        },
        ensure_ascii=False,
        indent=2,
    ))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
