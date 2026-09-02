from __future__ import annotations

import hashlib
import shutil
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
        redecode: bool = False,
        auto_cleanup: bool = False,
    ) -> None:
        for field, value in (
            ("source", source),
            ("workspace_root", workspace_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Ftrace Provider {field} must be a Path")
        if type(clock_domain) is not str:
            raise TypeError("Ftrace Provider clock_domain must be a string")
        if type(redecode) is not bool:
            raise TypeError("Ftrace Provider redecode must be a bool")
        if type(auto_cleanup) is not bool:
            raise TypeError("Ftrace Provider auto_cleanup must be a bool")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("Ftrace Provider clock_domain must be non-empty")
        if not workspace_root.is_dir():
            raise RuntimeError("Ftrace Provider workspace_root must be a directory")

        self._source = source
        self._clock_domain = clock_domain
        self._fusion: dp.DataFusionProvider | None = None
        self._decode_report = text_ftrace.DecodeReport(unsupported_event_names=())
        self._tables: tuple[str, ...] = ()
        self._auto_cleanup = False
        self._finished = False
        try:
            if not self._source.is_file():
                raise RuntimeError("Ftrace Provider source must be an existing file")
            source = self._source.resolve(strict=True)
            cache_root = workspace_root / ".ftrace-cache"
            cache_root.mkdir(exist_ok=True)
            if cache_root.is_symlink() or not cache_root.is_dir():
                raise RuntimeError(
                    "Ftrace Provider cache root must be a regular directory"
                )
            self._catalog_root = cache_root / _content_hash(source)
            if redecode:
                _remove_catalog(self._catalog_root)
            self._fusion, self._decode_report = self._open_or_rebuild_catalog(source)
            self._auto_cleanup = auto_cleanup
        except BaseException:
            self._finished = True
            raise

    def _open_or_rebuild_catalog(
        self, source: Path
    ) -> tuple[dp.DataFusionProvider, text_ftrace.DecodeReport]:
        if self._catalog_root.exists():
            try:
                return self._open_catalog()
            except _ClockDomainMismatch:
                raise
            except Exception:  # noqa: BLE001 - 任意准入失败都表示缓存不可用。
                _cleanup_catalog(self._catalog_root)
        return self._convert_and_open_catalog(source)

    def _convert_and_open_catalog(
        self, source: Path
    ) -> tuple[dp.DataFusionProvider, text_ftrace.DecodeReport]:
        """把当前来源完整转换为 Provider 管理的 Parquet Catalog。"""
        try:
            catalog_root = self._catalog_root.resolve(strict=False)
            try:
                text_ftrace.decode(source, catalog_root, self._clock_domain)
            except text_ftrace.DecodeError as error:
                raise RuntimeError(f"Ftrace Provider decode failed: {error}") from error
            if not catalog_root.is_dir() or catalog_root.is_symlink():
                raise RuntimeError(
                    "Ftrace Provider did not produce a regular catalog directory"
                )
            return self._open_catalog()
        except _ClockDomainMismatch:
            raise
        except OSError:
            _cleanup_catalog(self._catalog_root)
            raise RuntimeError("Ftrace Provider decode failed") from None
        except BaseException:
            _cleanup_catalog(self._catalog_root)
            raise

    def _open_catalog(
        self,
    ) -> tuple[dp.DataFusionProvider, text_ftrace.DecodeReport]:
        catalog = dp.open(root=self._catalog_root)
        relations = set(catalog.tables)
        self._tables = tuple(sorted(relations))
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
        fusion = dp.DataFusionProvider(catalog=catalog)
        if text_ftrace.EVENT_RELATION in relations:
            domains = {
                row["clock_domain"]
                for row in fusion.query(
                    f"SELECT DISTINCT clock_domain FROM {text_ftrace.EVENT_RELATION}"
                ).to_rows()
            }
            if domains != {self._clock_domain}:
                raise _ClockDomainMismatch(
                    "Ftrace Provider cached clock_domain does not match the request"
                )
        unsupported_event_names: tuple[str, ...] = ()
        if text_ftrace.UNSUPPORTED_EVENT_RELATION in relations:
            unsupported_event_names = tuple(
                row["event_name"]
                for row in fusion.query(
                    "SELECT event_name FROM "
                    f"{text_ftrace.UNSUPPORTED_EVENT_RELATION} "
                    "ORDER BY event_name"
                ).to_rows()
            )
            if unsupported_event_names != tuple(sorted(set(unsupported_event_names))):
                raise RuntimeError(
                    "Ftrace Provider unsupported event report is not sorted and unique"
                )
        return fusion, text_ftrace.DecodeReport(
            unsupported_event_names=unsupported_event_names
        )

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
        fusion = self._fusion
        if fusion is None:
            raise RuntimeError("Ftrace Provider is finished")
        return fusion.query(sql, params=params)

    def finish(self) -> None:
        if self._finished:
            return
        self._finished = True
        self._fusion = None
        if self._auto_cleanup:
            _remove_catalog(self._catalog_root)

    def __del__(self) -> None:
        try:
            self.finish()
        except Exception:
            pass


def _content_hash(source: Path) -> str:
    with source.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _remove_catalog(catalog_root: Path) -> None:
    if not catalog_root.exists() and not catalog_root.is_symlink():
        return
    if catalog_root.is_symlink() or catalog_root.is_file():
        catalog_root.unlink()
    else:
        shutil.rmtree(catalog_root)


def _cleanup_catalog(catalog_root: Path) -> None:
    try:
        _remove_catalog(catalog_root)
    except OSError:
        pass


class _ClockDomainMismatch(RuntimeError):
    pass
