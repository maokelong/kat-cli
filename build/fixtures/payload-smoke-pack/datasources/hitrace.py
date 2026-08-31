from __future__ import annotations

from pathlib import Path

from kat import dataprovider as dp
from kat_datasource import hitrace


class HitraceProvider:
    """Minimal PACK-owned Provider used to exercise the shipped decode API."""

    def __init__(self, source: Path) -> None:
        self._source = source
        self._query_provider: dp.DataFusionProvider | None = None

    def decode(self, destination: Path) -> None:
        report = hitrace.decode(self._source, destination)
        if report.unsupported_plugins or report.unsupported_section_types:
            raise ValueError("the Payload smoke fixture contains unsupported Hitrace content")
        catalog = dp.open(root=destination)
        self._query_provider = dp.DataFusionProvider(catalog=catalog)

    def query(self, sql: str) -> dp.Table:
        if self._query_provider is None:
            raise RuntimeError("decode must be called before query")
        return self._query_provider.query(sql)
