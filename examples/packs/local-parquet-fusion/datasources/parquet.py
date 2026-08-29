from collections.abc import Mapping
from pathlib import Path

from kat import datasource as ds


class LocalParquetProvider:
    """PACK 自有、只查询显式 Parquet 表的普通 Provider。"""

    def __init__(
        self,
        *,
        tables: Mapping[str, Path],
    ) -> None:
        catalog = ds.open(tables=tables)
        self._fusion = ds.DataFusionProvider(catalog=catalog)

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> ds.Table:
        return self._fusion.query(sql, params=params)
