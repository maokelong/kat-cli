from __future__ import annotations

from collections.abc import Mapping
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory

from kat import dataprovider as dp


REQUIRED_RELATIONS = frozenset(
    {
        "text_ftrace_header",
        "text_ftrace_event_occurrence",
        "text_ftrace_event",
    }
)


class FtraceProvider:
    """查询一份文本 Ftrace 所提供的类型化关系。"""

    def __init__(
        self,
        *,
        source: Path,
        clock_domain: str,
        workspace_root: Path,
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
        if type(auto_cleanup) is not bool:
            raise TypeError("Ftrace Provider auto_cleanup must be a bool")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("Ftrace Provider clock_domain must be non-empty")
        if not workspace_root.is_dir():
            raise RuntimeError("Ftrace Provider workspace_root must be a directory")

        self._source = source
        self._clock_domain = clock_domain
        self._workspace: TemporaryDirectory[str] | None = None
        try:
            if not self._source.is_file():
                raise RuntimeError("Ftrace Provider source must be an existing file")
            source = self._source.resolve(strict=True)
            if auto_cleanup:
                self._workspace = TemporaryDirectory(
                    prefix="ftrace-",
                    dir=workspace_root,
                )
                self._catalog_root = Path(self._workspace.name) / "catalog"
                self._fusion = self._convert_and_open_catalog(source)
            else:
                cache_root = workspace_root / ".ftrace2parquet-cache"
                cache_root.mkdir(exist_ok=True)
                if cache_root.is_symlink() or not cache_root.is_dir():
                    raise RuntimeError(
                        "Ftrace Provider cache root must be a regular directory"
                    )
                self._catalog_root = cache_root / _content_hash(source)
                self._fusion = self._open_or_rebuild_catalog(source)
        except BaseException:
            if self._workspace is not None:
                self._workspace.cleanup()
            raise

    def _open_or_rebuild_catalog(self, source: Path) -> dp.DataFusionProvider:
        if self._catalog_root.exists():
            try:
                return self._open_catalog()
            except _ClockDomainMismatch:
                raise
            except Exception:
                _remove_catalog(self._catalog_root)
        return self._convert_and_open_catalog(source)

    def _convert_and_open_catalog(self, source: Path) -> dp.DataFusionProvider:
        """把当前来源完整转换为 Provider 管理的 Parquet Catalog。"""
        try:
            executable = _resolve_executable()
            catalog_root = self._catalog_root.resolve(strict=False)

            completed = subprocess.run(
                [
                    str(executable),
                    "--input",
                    str(source),
                    "--output",
                    str(catalog_root),
                    "--clock-domain",
                    self._clock_domain,
                ],
                cwd=catalog_root.parent,
                shell=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if completed.returncode != 0:
                if (
                    self._workspace is None
                    and catalog_root.is_dir()
                    and not catalog_root.is_symlink()
                ):
                    try:
                        return self._open_catalog()
                    except _ClockDomainMismatch:
                        raise
                    except Exception:
                        pass
                raise RuntimeError("Ftrace Provider decode failed")
            if not catalog_root.is_dir() or catalog_root.is_symlink():
                raise RuntimeError(
                    "Ftrace Provider did not produce a regular catalog directory"
                )
            return self._open_catalog()
        except _ClockDomainMismatch:
            raise
        except OSError:
            _remove_catalog(self._catalog_root)
            raise RuntimeError("Ftrace Provider decode failed") from None
        except BaseException:
            _remove_catalog(self._catalog_root)
            raise

    def _open_catalog(self) -> dp.DataFusionProvider:
        catalog = dp.open(root=self._catalog_root)
        missing = REQUIRED_RELATIONS.difference(catalog.tables)
        if missing:
            raise RuntimeError(
                "Ftrace Provider output is missing required relations: "
                + ", ".join(sorted(missing))
            )
        fusion = dp.DataFusionProvider(catalog=catalog)
        domains = {
            row["clock_domain"]
            for row in fusion.query(
                "SELECT DISTINCT clock_domain FROM text_ftrace_event"
            ).to_rows()
        }
        if domains != {self._clock_domain}:
            raise _ClockDomainMismatch(
                "Ftrace Provider cached clock_domain does not match the request"
            )
        return fusion

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        return self._fusion.query(sql, params=params)


def _resolve_executable() -> Path:
    value = os.environ.get("KAT_FTRACE2PARQUET_EXECUTABLE")
    if not value:
        raise RuntimeError(
            "KAT_FTRACE2PARQUET_EXECUTABLE must identify the approved converter"
        )
    executable = Path(value)
    if not executable.is_file():
        raise RuntimeError("ftrace2parquet executable must be an existing file")
    return executable.resolve(strict=True)


def _content_hash(source: Path) -> str:
    digest = hashlib.sha256()
    with source.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _remove_catalog(catalog_root: Path) -> None:
    try:
        if catalog_root.is_symlink():
            catalog_root.unlink(missing_ok=True)
        else:
            shutil.rmtree(catalog_root, ignore_errors=True)
    except OSError:
        pass


class _ClockDomainMismatch(RuntimeError):
    pass
