from __future__ import annotations

from typing import Any


class Kat:
    def __init__(self, *, ctx: Any, run_dir: str | None = None, logger: Any = None) -> None:
        self.ctx = ctx
        self.run_dir = run_dir
        self._logger = logger

    def sql(self, sql: str, **params: Any) -> Any:
        rendered = _bind_sql_params(sql, params)
        return self.ctx.sql(rendered)

    def log(self, level: str, message: str, **fields: Any) -> None:
        if self._logger is not None:
            self._logger(level, message, fields)


def _bind_sql_params(sql: str, params: dict[str, Any]) -> str:
    rendered = sql
    for name, value in params.items():
        rendered = rendered.replace(f":{name}", _sql_literal(value))
    return rendered


def _sql_literal(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int | float):
        return str(value)
    escaped = str(value).replace("'", "''")
    return f"'{escaped}'"
