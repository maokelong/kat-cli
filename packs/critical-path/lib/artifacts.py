from __future__ import annotations

from typing import Any

from .model import CriticalPathResult

Schema = list[tuple[str, str]]

TARGET_WINDOW_SCHEMA: Schema = [
    ("root_itid", "BIGINT"),
    ("start_ts", "BIGINT"),
    ("end_ts", "BIGINT"),
    ("duration_ns", "BIGINT"),
]

PATH_NODE_SCHEMA: Schema = [
    ("node_id", "BIGINT"),
    ("depth", "BIGINT"),
    ("itid", "BIGINT"),
    ("tid", "BIGINT"),
    ("thread_name", "TEXT"),
    ("start_ts", "BIGINT"),
    ("end_ts", "BIGINT"),
    ("duration_ns", "BIGINT"),
    ("state", "TEXT"),
    ("classification", "TEXT"),
    ("reason", "TEXT"),
    ("blocked_context", "TEXT"),
    ("evidence_name", "TEXT"),
    ("source_state_id", "BIGINT"),
]

PATH_EDGE_SCHEMA: Schema = [
    ("edge_id", "BIGINT"),
    ("relation", "TEXT"),
    ("from_node_id", "BIGINT"),
    ("to_node_id", "BIGINT"),
    ("from_itid", "BIGINT"),
    ("to_itid", "BIGINT"),
    ("start_ts", "BIGINT"),
    ("end_ts", "BIGINT"),
    ("duration_ns", "BIGINT"),
    ("classification", "TEXT"),
    ("evidence_name", "TEXT"),
    ("source_wakeup_id", "BIGINT"),
]

CLASSIFICATION_SCHEMA: Schema = [
    ("classification", "TEXT"),
    ("node_count", "BIGINT"),
    ("total_duration_ns", "BIGINT"),
    ("max_depth", "BIGINT"),
]

UNCERTAINTY_SCHEMA: Schema = [
    ("code", "TEXT"),
    ("message", "TEXT"),
    ("itid", "BIGINT"),
    ("depth", "BIGINT"),
    ("start_ts", "BIGINT"),
    ("end_ts", "BIGINT"),
]

EVIDENCE_SCHEMA: Schema = [
    ("fact_kind", "TEXT"),
    ("node_count", "BIGINT"),
    ("edge_count", "BIGINT"),
    ("uncertainty_count", "BIGINT"),
    ("max_depth", "BIGINT"),
]


def artifact_queries(kat: Any, result: CriticalPathResult) -> dict[str, Any]:
    return {
        "target_window": kat.query(_values_sql(_target_window_rows(result), TARGET_WINDOW_SCHEMA)),
        "path_nodes": kat.query(_values_sql(_path_node_rows(result), PATH_NODE_SCHEMA)),
        "path_edges": kat.query(_values_sql(_path_edge_rows(result), PATH_EDGE_SCHEMA)),
        "critical_classification": kat.query(_values_sql(result.classification_rows(), CLASSIFICATION_SCHEMA)),
        "uncertainties": kat.query(_values_sql(_uncertainty_rows(result), UNCERTAINTY_SCHEMA)),
        "critical_path_evidence": kat.query(_values_sql(result.evidence_rows(), EVIDENCE_SCHEMA)),
    }


def _target_window_rows(result: CriticalPathResult) -> list[dict[str, Any]]:
    window = result.window
    return [
        {
            "root_itid": window.root_itid,
            "start_ts": window.start_ts,
            "end_ts": window.end_ts,
            "duration_ns": window.end_ts - window.start_ts,
        }
    ]


def _path_node_rows(result: CriticalPathResult) -> list[dict[str, Any]]:
    return [
        {
            "node_id": node.node_id,
            "depth": node.depth,
            "itid": node.itid,
            "tid": node.tid,
            "thread_name": node.thread_name,
            "start_ts": node.start_ts,
            "end_ts": node.end_ts,
            "duration_ns": node.duration_ns,
            "state": node.state,
            "classification": node.classification,
            "reason": node.reason,
            "blocked_context": node.blocked_context,
            "evidence_name": node.evidence_name,
            "source_state_id": node.source_state_id,
        }
        for node in result.nodes
    ]


def _path_edge_rows(result: CriticalPathResult) -> list[dict[str, Any]]:
    return [
        {
            "edge_id": edge.edge_id,
            "relation": edge.relation,
            "from_node_id": edge.from_node_id,
            "to_node_id": edge.to_node_id,
            "from_itid": edge.from_itid,
            "to_itid": edge.to_itid,
            "start_ts": edge.start_ts,
            "end_ts": edge.end_ts,
            "duration_ns": edge.duration_ns,
            "classification": edge.classification,
            "evidence_name": edge.evidence_name,
            "source_wakeup_id": edge.source_wakeup_id,
        }
        for edge in result.edges
    ]


def _uncertainty_rows(result: CriticalPathResult) -> list[dict[str, Any]]:
    return [
        {
            "code": uncertainty.code,
            "message": uncertainty.message,
            "itid": uncertainty.itid,
            "depth": uncertainty.depth,
            "start_ts": uncertainty.start_ts,
            "end_ts": uncertainty.end_ts,
        }
        for uncertainty in result.uncertainties
    ]


def _values_sql(rows: list[dict[str, Any]], schema: Schema) -> str:
    if not rows:
        select_list = ",\n  ".join(f"CAST(NULL AS {column_type}) AS {name}" for name, column_type in schema)
        return f"SELECT\n  {select_list}\nWHERE 1 = 0"

    columns = ", ".join(name for name, _ in schema)
    values = ",\n  ".join(
        "(" + ", ".join(_sql_literal(row.get(name)) for name, _ in schema) + ")"
        for row in rows
    )
    casts = ",\n  ".join(f"CAST({name} AS {column_type}) AS {name}" for name, column_type in schema)
    return f"WITH data({columns}) AS (\n  VALUES\n  {values}\n)\nSELECT\n  {casts}\nFROM data"


def _sql_literal(value: Any) -> str:
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, (int, float)):
        return str(value)
    return "'" + str(value).replace("'", "''") + "'"
