from pathlib import Path

from kat import datasource as ds


class LocalParquetProvider:
    """PACK 自有、显式打开线程归属 Parquet 的普通 Provider。"""

    def __init__(self, *, thread_placement: Path) -> None:
        self._catalog = ds.open(
            tables={"thread_placement": thread_placement},
        )

    @property
    def catalog(self) -> ds.Catalog:
        return self._catalog
