"""Reusable text Ftrace datasource Provider for KAT PACKs."""

from __future__ import annotations

import unicodedata
from collections.abc import Mapping
from pathlib import Path
from typing import TYPE_CHECKING

from .._provider import provider

if TYPE_CHECKING:
    from kat_datasource import text_ftrace

from . import _fusion, _parquet
from ._table import Table

_WINDOWS_DEVICE_NAMES = frozenset(
    {"con", "prn", "aux", "nul"}
    | {f"com{index}" for index in range(1, 10)}
    | {f"lpt{index}" for index in range(1, 10)}
)
_WINDOWS_FORBIDDEN_CHARACTERS = frozenset('<>:"/\\|?*')


@provider(
    name="ftrace-text",
    description="将 tracefs 文本解码为可重复查询的类型化关系。",
    guide="providers/ftrace.md",
)
class FtraceProvider:
    """Decode and query one text Ftrace through a reusable Parquet catalog."""

    def __init__(
        self, *, source: Path, clock_domain: str, workspace_root: Path
    ) -> None:
        from kat_datasource import text_ftrace

        for field, value in (("source", source), ("workspace_root", workspace_root)):
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
        self._query_provider: _fusion.DataFusionProvider
        self._decode_report = text_ftrace.DecodeReport(unsupported_event_names=())
        self._tables: tuple[str, ...] = ()
        self._catalog_root = workspace_root.resolve(strict=True) / _source_stem(source)
        if _path_exists(self._catalog_root):
            self._open_catalog()
            return
        if not source.is_file():
            raise RuntimeError("Ftrace Provider source must be an existing file")
        source = source.resolve(strict=True)
        try:
            self._decode(source)
        except text_ftrace.DecodeError as error:
            if _path_exists(self._catalog_root):
                self._open_catalog()
                return
            raise RuntimeError(f"Ftrace Provider decode failed: {error}") from error
        self._open_catalog()

    def _decode(self, source: Path) -> None:
        from kat_datasource import text_ftrace

        text_ftrace.decode(source, self._catalog_root, self._clock_domain)
        if not self._catalog_root.is_dir() or self._catalog_root.is_symlink():
            raise RuntimeError(
                "Ftrace Provider did not produce a regular catalog directory"
            )

    def _open_catalog(self) -> None:
        from kat_datasource import text_ftrace

        catalog = _parquet.open(root=self._catalog_root)
        relations = catalog.tables
        query_provider = _fusion.DataFusionProvider(catalog=catalog)
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
        self._query_provider = query_provider
        self._decode_report = text_ftrace.DecodeReport(
            unsupported_event_names=unsupported_event_names
        )
        self._tables = relations

    @property
    def decode_report(self) -> text_ftrace.DecodeReport:
        return self._decode_report

    @property
    def tables(self) -> tuple[str, ...]:
        return self._tables

    def query(self, sql: str, *, params: Mapping[str, object] | None = None) -> Table:
        return self._query_provider.query(sql, params=params)


def _source_stem(source: Path) -> str:
    stem = source.stem
    device_name = stem.split(".", 1)[0].casefold()
    if (
        not stem
        or stem in {".", ".."}
        or stem.endswith((".", " "))
        or device_name in _WINDOWS_DEVICE_NAMES
        or any(
            character in _WINDOWS_FORBIDDEN_CHARACTERS
            or unicodedata.category(character) == "Cc"
            for character in stem
        )
    ):
        raise ValueError(f"invalid Ftrace Provider source stem: {stem!r}")
    return stem


def _path_exists(path: Path) -> bool:
    return path.exists() or path.is_symlink()
