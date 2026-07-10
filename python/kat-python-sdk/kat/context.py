from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from datafusion import DataFrame, SessionContext


class Kat:
    def __init__(
        self,
        *,
        ctx: SessionContext,
        run_dir: str | None = None,
        logger: Any = None,
    ) -> None:
        self.ctx = ctx
        self.run_dir = run_dir
        self._logger = logger

    def sql(self, sql: str, **params: Any) -> DataFrame:
        return self.ctx.sql(sql, param_values=params or None)

    def log(self, level: str, message: str, **fields: Any) -> None:
        if self._logger is not None:
            self._logger(level, message, fields)
