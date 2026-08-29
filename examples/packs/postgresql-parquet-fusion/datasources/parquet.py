from collections.abc import Mapping
from pathlib import Path

from kat import datasource as ds


SCHED_SWITCH_SCHEMA = ds.Schema(
    {
        "sched_switch": {
            "cpu": int | None,
            "next_thread_id": int | None,
            "timestamp": int | None,
        }
    }
)


class LocalParquetProvider:
    """PACK 自有、只查询显式 sched_switch 表的普通 Provider。"""

    def __init__(self, *, sched_switch: Path) -> None:
        self._catalog = ds.open(
            SCHED_SWITCH_SCHEMA,
            tables={"sched_switch": sched_switch},
        )

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> ds.Table:
        return self._catalog.query(sql, params=params)
