from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
import shutil
import subprocess

from kat import dataprovider as dp


REQUIRED_RELATIONS = frozenset(
    {
        "text_ftrace_header",
        "text_ftrace_event_occurrence",
        "text_ftrace_event",
    }
)

SUMMARY_SQL = """
SELECT
    h.tracer,
    h.cpu_count,
    h.entries_written AS source_event_count,
    COUNT(e._kat_row_id) AS supported_event_count
FROM text_ftrace_header h
CROSS JOIN text_ftrace_event e
GROUP BY h.tracer, h.cpu_count, h.entries_written
"""


class Ftrace2ParquetProvider:
    """调用 Rust 转换器并查询其类型化 Parquet 关系。"""

    def __init__(
        self,
        *,
        source: Path,
        executable: Path,
        catalog_root: Path,
        clock_domain: str,
    ) -> None:
        for field, value in (
            ("source", source),
            ("executable", executable),
            ("catalog_root", catalog_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Ftrace2Parquet {field} must be a Path")
        if type(clock_domain) is not str:
            raise TypeError("Ftrace2Parquet clock_domain must be a string")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("Ftrace2Parquet clock_domain must be non-empty")

        self._source = source
        self._executable = executable
        self._catalog_root = catalog_root
        self._clock_domain = clock_domain
        self._fusion = self._initialize()

    def _initialize(self) -> dp.DataFusionProvider:
        """把当前来源完整转换为本 Provider 独占的 Parquet Catalog。"""
        try:
            # catalog_root 是调用方明确交付的独占 leaf；保留词法路径，避免删除穿过
            # resolve 后的 symlink 扩大到 workspace 之外。
            _remove_owned_catalog(self._catalog_root)
            if not self._source.is_file():
                raise RuntimeError("Ftrace2Parquet source must be an existing file")
            if not self._executable.is_file():
                raise RuntimeError(
                    "Ftrace2Parquet executable must be an existing file"
                )

            source = self._source.resolve(strict=True)
            executable = self._executable.resolve(strict=True)
            catalog_root = self._catalog_root.resolve(strict=False)
            if not catalog_root.parent.is_dir():
                raise RuntimeError("Ftrace2Parquet catalog parent must exist")

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
                raise RuntimeError("Ftrace2Parquet decode failed")
            if not catalog_root.is_dir() or catalog_root.is_symlink():
                raise RuntimeError(
                    "Ftrace2Parquet did not produce a regular catalog directory"
                )

            catalog = dp.open(root=catalog_root)
            missing = REQUIRED_RELATIONS.difference(catalog.tables)
            if missing:
                raise RuntimeError(
                    "Ftrace2Parquet output is missing required relations: "
                    + ", ".join(sorted(missing))
                )
            fusion = dp.DataFusionProvider(catalog=catalog)
        except OSError:
            _cleanup_owned_catalog(self._catalog_root)
            raise RuntimeError("Ftrace2Parquet decode failed") from None
        except BaseException:
            _cleanup_owned_catalog(self._catalog_root)
            raise

        return fusion

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        return self._fusion.query(sql, params=params)


def _remove_owned_catalog(catalog_root: Path) -> None:
    if catalog_root.is_symlink() or catalog_root.is_file():
        catalog_root.unlink()
    elif catalog_root.exists():
        shutil.rmtree(catalog_root)


def _cleanup_owned_catalog(catalog_root: Path) -> None:
    try:
        _remove_owned_catalog(catalog_root)
    except OSError:
        pass
