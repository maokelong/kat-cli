from collections.abc import Mapping
from pathlib import Path

from kat import datasource as ds


EVENTS_SCHEMA = ds.Schema(
    {
        "events": {
            "event_id": int | None,
            "owner_id": int | None,
            "score": int | None,
        },
        "labels": {
            "event_id": int | None,
            "label": str | None,
        },
    }
)

OWNERS_SCHEMA = ds.Schema(
    {
        "owners": {
            "owner_id": int | None,
            "owner_name": str | None,
        }
    }
)


class LocalParquetProvider:
    """PACK 自有、只查询显式 Parquet 表的普通 Provider。"""

    def __init__(
        self,
        *,
        schema: ds.Schema,
        tables: Mapping[str, Path],
    ) -> None:
        self._catalog = ds.open(schema, tables=tables)

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> ds.Table:
        return self._catalog.query(sql, params=params)
