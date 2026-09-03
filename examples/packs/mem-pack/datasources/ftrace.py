from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path

from kat_datasource import text_ftrace

import kat
from kat import dataprovider as dp


@kat.provider(
    name="ftrace-text",
    description="将 tracefs 文本解码为可重复查询的类型化关系。",
    guide="providers/ftrace.md",
)
class FtraceProvider:
    """查询一份文本 Ftrace 所提供的类型化关系。"""

    def __init__(
        self,
        *,
        source: Path,
        clock_domain: str,
        workspace_root: Path,
    ) -> None:
        for field, value in (
            ("source", source),
            ("workspace_root", workspace_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Ftrace Provider {field} must be a Path")
        if type(clock_domain) is not str:
            raise TypeError("Ftrace Provider clock_domain must be a string")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("Ftrace Provider clock_domain must be non-empty")
        if not workspace_root.is_dir():
            raise RuntimeError("Ftrace Provider workspace_root must be a directory")

        self._clock_domain = clock_domain
        self._query_provider: dp.DataFusionProvider
        self._decode_report = text_ftrace.DecodeReport(unsupported_event_names=())
        self._tables: tuple[str, ...] = ()

        if not source.is_file():
            raise RuntimeError("Ftrace Provider source must be an existing file")
        source = source.resolve(strict=True)
        self._catalog_root = workspace_root.resolve(strict=True) / source.name
        if self._catalog_root.is_symlink() or self._catalog_root.is_file():
            raise RuntimeError(
                "Ftrace Provider catalog path must be a directory or absent"
            )
        if any(self._catalog_root.glob("*.parquet")):
            self._open_catalog()
            return
        self._decode(source)
        self._open_catalog()

    def _decode(self, source: Path) -> None:
        """把来源转换到按文件名确定的 Parquet Catalog。"""
        if self._catalog_root.exists():
            try:
                self._catalog_root.rmdir()
            except OSError:
                raise RuntimeError(
                    "Ftrace Provider catalog without Parquet must be empty"
                ) from None
        try:
            text_ftrace.decode(source, self._catalog_root, self._clock_domain)
        except text_ftrace.DecodeError as error:
            raise RuntimeError(f"Ftrace Provider decode failed: {error}") from error
        if not self._catalog_root.is_dir() or self._catalog_root.is_symlink():
            raise RuntimeError(
                "Ftrace Provider did not produce a regular catalog directory"
            )

    def _open_catalog(
        self,
    ) -> None:
        catalog = dp.open(root=self._catalog_root)
        relations = set(catalog.tables)
        if text_ftrace.HEADER_RELATION not in relations:
            raise RuntimeError(
                f"Ftrace Provider output is missing {text_ftrace.HEADER_RELATION}"
            )
        if (text_ftrace.OCCURRENCE_RELATION in relations) != (
            text_ftrace.EVENT_RELATION in relations
        ):
            raise RuntimeError(
                "Ftrace Provider output must contain both "
                f"{text_ftrace.OCCURRENCE_RELATION} and "
                f"{text_ftrace.EVENT_RELATION}"
            )
        query_provider = dp.DataFusionProvider(catalog=catalog)
        if text_ftrace.EVENT_RELATION in relations:
            domains = {
                row["clock_domain"]
                for row in query_provider.query(
                    f"SELECT DISTINCT clock_domain FROM {text_ftrace.EVENT_RELATION}"
                ).to_rows()
            }
            if domains != {self._clock_domain}:
                raise RuntimeError(
                    "Ftrace Provider cached clock_domain does not match the request"
                )
        unsupported_event_names: tuple[str, ...] = ()
        if text_ftrace.UNSUPPORTED_EVENT_RELATION in relations:
            unsupported_event_names = tuple(
                row["event_name"]
                for row in query_provider.query(
                    "SELECT event_name FROM "
                    f"{text_ftrace.UNSUPPORTED_EVENT_RELATION} "
                    "ORDER BY event_name"
                ).to_rows()
            )
            if unsupported_event_names != tuple(sorted(set(unsupported_event_names))):
                raise RuntimeError(
                    "Ftrace Provider unsupported event report is not sorted and unique"
                )
        decode_report = text_ftrace.DecodeReport(
            unsupported_event_names=unsupported_event_names
        )
        self._query_provider = query_provider
        self._decode_report = decode_report
        self._tables = tuple(sorted(relations))

    @property
    def decode_report(self) -> text_ftrace.DecodeReport:
        return self._decode_report

    @property
    def tables(self) -> tuple[str, ...]:
        return self._tables

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        return self._query_provider.query(sql, params=params)
