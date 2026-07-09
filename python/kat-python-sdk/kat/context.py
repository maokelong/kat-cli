from __future__ import annotations

import re
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
    if not params:
        return sql

    pattern = re.compile(r":([A-Za-z_][A-Za-z0-9_]*)")

    def replace(match: re.Match[str]) -> str:
        name = match.group(1)
        value = params.get(name)
        if name not in params:
            return match.group(0)
        return _sql_literal(value)

    return pattern.sub(replace, sql)


def _sql_literal(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int | float):
        return str(value)
    escaped = str(value).replace("'", "''")
    return f"'{escaped}'"
