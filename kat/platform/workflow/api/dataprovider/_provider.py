from __future__ import annotations

from collections.abc import Mapping
from typing import Protocol

from ._table import Table


class Provider(Protocol):
    """可由分析代码查询并返回 eager Table 的最小合同。"""

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> Table: ...
