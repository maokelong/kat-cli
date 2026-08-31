from __future__ import annotations

from collections.abc import Mapping
import os
from pathlib import Path
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

        self._source = source
        self._clock_domain = clock_domain
        self._workspace = None
        try:
            self._executable = _resolve_executable()
            self._workspace = TemporaryDirectory(
                prefix="ftrace-",
                dir=workspace_root,
            )
            self._catalog_root = Path(self._workspace.name) / "catalog"
            catalog = self._convert_and_open_catalog()
            self._fusion = dp.DataFusionProvider(catalog=catalog)
        except BaseException:
            if self._workspace is not None:
                self._workspace.cleanup()
            raise

    def _convert_and_open_catalog(self) -> dp.Catalog:
        """把当前来源完整转换为本 Provider 独占的 Parquet Catalog。"""
        try:
            if not self._source.is_file():
                raise RuntimeError("Ftrace Provider source must be an existing file")

            source = self._source.resolve(strict=True)
            catalog_root = self._catalog_root.resolve(strict=False)

            completed = subprocess.run(
                [
                    str(self._executable),
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
                raise RuntimeError("Ftrace Provider decode failed")
            if not catalog_root.is_dir() or catalog_root.is_symlink():
                raise RuntimeError(
                    "Ftrace Provider did not produce a regular catalog directory"
                )

            catalog = dp.open(root=catalog_root)
            missing = REQUIRED_RELATIONS.difference(catalog.tables)
            if missing:
                raise RuntimeError(
                    "Ftrace Provider output is missing required relations: "
                    + ", ".join(sorted(missing))
                )
        except OSError:
            raise RuntimeError("Ftrace Provider decode failed") from None

        return catalog

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
