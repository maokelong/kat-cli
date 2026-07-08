from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Protocol


class RuntimeChannel(Protocol):
    def query(self, sql: str, params: Mapping[str, Any]) -> Any:
        ...

    def preview(self, query_id: str, limit: int) -> list[dict[str, Any]]:
        ...

    def rows(self, query_id: str, max_rows: int) -> list[dict[str, Any]]:
        ...

    def log(self, level: str, message: str, fields: Mapping[str, Any]) -> None:
        ...


_runtime_channel: RuntimeChannel | None = None


@dataclass(frozen=True)
class QueryResult:
    query_id: str
    _channel: RuntimeChannel

    def preview(self, limit: int = 20) -> list[dict[str, Any]]:
        if limit <= 0:
            raise ValueError("limit must be positive")
        return _normalize_rows(self._channel.preview(self.query_id, limit))

    def rows(self, max_rows: int) -> list[dict[str, Any]]:
        if max_rows <= 0:
            raise ValueError("max_rows must be positive")
        return _normalize_rows(self._channel.rows(self.query_id, max_rows))


def bind_runtime(channel: RuntimeChannel) -> None:
    global _runtime_channel
    _runtime_channel = channel


def reset_runtime() -> None:
    global _runtime_channel
    _runtime_channel = None


def query(sql: str, **params: Any) -> QueryResult:
    channel = _require_runtime()
    response = channel.query(sql, params)
    if isinstance(response, QueryResult):
        return response
    query_id = _extract_query_id(response)
    return QueryResult(query_id=query_id, _channel=channel)


def log(message: str, level: str = "info", **fields: Any) -> None:
    _require_runtime().log(level, message, fields)


def validate_workflow_return(value: Any) -> dict[str, QueryResult]:
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise TypeError("workflow return value must be dict[str, QueryResult]")
    normalized: dict[str, QueryResult] = {}
    for name, result in value.items():
        if not isinstance(name, str) or not name:
            raise TypeError("workflow artifact names must be non-empty strings")
        if not isinstance(result, QueryResult):
            raise TypeError("workflow artifact values must be QueryResult")
        normalized[name] = result
    return normalized


def _require_runtime() -> RuntimeChannel:
    if _runtime_channel is None:
        raise RuntimeError("kat runtime is not bound")
    return _runtime_channel


def _extract_query_id(response: Any) -> str:
    if isinstance(response, str):
        return response
    if isinstance(response, Mapping):
        for key in ("query_id", "queryId", "id"):
            value = response.get(key)
            if value:
                return str(value)
    value = getattr(response, "query_id", None)
    if value:
        return str(value)
    value = getattr(response, "queryId", None)
    if value:
        return str(value)
    raise RuntimeError("runtime query response did not include query_id")


def _normalize_rows(rows: Any) -> list[dict[str, Any]]:
    if rows is None:
        return []
    normalized: list[dict[str, Any]] = []
    for row in rows:
        if isinstance(row, dict):
            normalized.append(dict(row))
        elif hasattr(row, "_asdict"):
            normalized.append(dict(row._asdict()))
        elif hasattr(row, "__dict__"):
            normalized.append(dict(row.__dict__))
        else:
            raise TypeError(f"cannot normalize row: {row!r}")
    return normalized
