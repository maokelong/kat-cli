from __future__ import annotations

__all__ = ["TraceStreamerProvider"]

from collections.abc import Mapping
from pathlib import Path
import sqlite3
import os
import stat
import subprocess
import tempfile
import unicodedata

import pyarrow as pa

import kat
from kat import dataprovider as dp
from kat.dataprovider import publish_materialization


_READ_ONLY_SQLITE_ACTIONS = frozenset(
    {
        sqlite3.SQLITE_FUNCTION,
        sqlite3.SQLITE_READ,
        sqlite3.SQLITE_RECURSIVE,
        sqlite3.SQLITE_SELECT,
    }
)


@kat.provider(
    name="trace-streamer-sqlite",
    description="解码 Trace Streamer 来源并以只读 SQLite SQL 查询。",
    guide="providers/trace-streamer-sqlite.md",
)
class TraceStreamerProvider:
    """解码或打开 Trace Streamer SQLite，并显式执行只读查询。

    Parameters:
        source: 待解码 Trace；不能与 sqlite_path 同时提供。
        executable: 调用方提供的 Trace Streamer 可执行文件。
        workspace_root: 解码物化根，通常为 ctx.datasource_root。
        sqlite_path: 已有 SQLite 的精确绝对路径；与全部解码参数互斥。

    Raises:
        ValueError: 路径、参数组合或数据库结构无效。
        RuntimeError: 外部解析器失败或没有产生完整物化。

    导入路径为 `kat_sdk.providers.trace_streamer.TraceStreamerProvider`。
    SDK 不附带外部 Trace Streamer；已有有效物化可直接复用。
    """

    def __init__(
        self,
        *,
        source: Path | None = None,
        executable: Path | None = None,
        workspace_root: Path | None = None,
        sqlite_path: str | Path | None = None,
    ) -> None:
        if sqlite_path is not None:
            if any(value is not None for value in (source, executable, workspace_root)):
                raise ValueError("SQLite and decode parameters are mutually exclusive")
            if not isinstance(sqlite_path, (str, Path)):
                raise TypeError("Trace Streamer SQLite path must be a string or Path")
            supplied = Path(sqlite_path)
            if not supplied.is_absolute():
                raise ValueError("Trace Streamer SQLite path must be absolute")
            _require_regular(supplied, directory=False)
            resolved = supplied.resolve(strict=True)
            if resolved != supplied:
                raise ValueError(
                    "Trace Streamer SQLite path must identify its exact file"
                )
            self._database = resolved
            _verify_database(resolved)
            return
        for name, value in (
            ("source", source),
            ("executable", executable),
            ("workspace_root", workspace_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Trace Streamer {name} must be a Path")
        assert (
            source is not None and executable is not None and workspace_root is not None
        )
        _require_regular(workspace_root, directory=True)
        root = workspace_root.resolve(strict=True)
        destination = root / _source_stem(source)
        self._database = destination / "trace.db"
        if os.path.lexists(destination):
            _verify_materialization(destination)
            return
        if not source.is_file():
            raise ValueError("Trace Streamer source must be an existing file")
        if not executable.is_file():
            raise ValueError("Trace Streamer executable must be an existing file")
        source = source.resolve(strict=True)
        executable = executable.resolve(strict=True)
        with tempfile.TemporaryDirectory(
            prefix=".trace-streamer-", dir=root
        ) as temporary:
            candidate = Path(temporary) / "decoded"
            candidate.mkdir()
            completed = subprocess.run(
                [str(executable), str(source), "-e", str(candidate / "trace.db")],
                cwd=executable.parent,
                shell=False,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            if completed.returncode != 0:
                detail = (completed.stderr + completed.stdout).decode(
                    "utf-8", errors="replace"
                )
                raise RuntimeError(
                    f"Trace Streamer decode failed (exit {completed.returncode}): {detail}"
                )
            _verify_materialization(candidate)
            try:
                publish_materialization(candidate, destination)
            except OSError:
                if not os.path.lexists(destination):
                    raise
                # 并发首次发布只接受完整胜者，不覆盖既有 Session 事实。
                _verify_materialization(destination)

    def query(
        self,
        sql: str,
        *,
        schema: pa.Schema,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        """执行单条只读 SQLite SQL，按显式 Arrow Schema 返回 Table。

        Parameters:
            sql: 当前 SQLite 的只读查询。
            schema: 完整输出列名、顺序与类型；查询列必须严格匹配。
            params: SQLite 命名绑定参数。

        Returns:
            与 schema 一致的 eager 表值。

        Raises:
            ValueError: 查询列与 schema 不一致。
            sqlite3.DatabaseError: 非只读操作、SQL 或数据库访问失败。
        """
        if type(sql) is not str or not sql.strip():
            raise TypeError("Trace Streamer SQL must be a non-empty string")
        if not isinstance(schema, pa.Schema):
            raise TypeError("Trace Streamer query schema must be a PyArrow schema")
        if params is None:
            bound: dict[str, object] = {}
        elif isinstance(params, Mapping) and all(type(name) is str for name in params):
            bound = dict(params.items())
        else:
            raise TypeError("Trace Streamer query parameters must be a named mapping")

        connection = sqlite3.connect(
            f"{self._database.as_uri()}?mode=ro",
            uri=True,
        )
        try:
            pragma = connection.execute("PRAGMA query_only = ON")
            pragma.close()
            connection.set_authorizer(_authorize_read_only)
            cursor = connection.execute(sql, bound)
            try:
                actual_columns = tuple(
                    column[0] for column in (cursor.description or ())
                )
                expected_columns = tuple(schema.names)
                if actual_columns != expected_columns:
                    raise ValueError(
                        "Trace Streamer query columns must exactly match schema order: "
                        f"expected {expected_columns!r}, got {actual_columns!r}"
                    )
                rows = cursor.fetchall()
            finally:
                cursor.close()
        finally:
            connection.close()

        return dp.Table.from_rows(
            (dict(zip(expected_columns, row, strict=True)) for row in rows),
            schema=schema,
        )


def _authorize_read_only(
    action_code: int,
    first: str | None,
    second: str | None,
    database: str | None,
    trigger: str | None,
) -> int:
    del first, second, database, trigger
    if action_code in _READ_ONLY_SQLITE_ACTIONS:
        return sqlite3.SQLITE_OK
    return sqlite3.SQLITE_DENY


def _require_regular(path: Path, *, directory: bool) -> None:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        raise ValueError(f"Trace Streamer path must exist: {path}") from None
    if getattr(
        metadata, "st_file_attributes", 0
    ) & stat.FILE_ATTRIBUTE_REPARSE_POINT or not (
        stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode)
    ):
        kind = "directory" if directory else "file"
        raise ValueError(f"Trace Streamer path must identify a regular {kind}: {path}")


def _verify_materialization(root: Path) -> None:
    _require_regular(root, directory=True)
    _verify_database(root / "trace.db")


def _verify_database(database: Path) -> None:
    _require_regular(database, directory=False)
    connection = sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True)
    try:
        if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise ValueError("Trace Streamer SQLite integrity check failed")
        if (
            connection.execute(
                "SELECT 1 FROM sqlite_schema "
                "WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' LIMIT 1"
            ).fetchone()
            is None
        ):
            raise ValueError("Trace Streamer SQLite contains no relations")
    finally:
        connection.close()


def _source_stem(source: Path) -> str:
    stem = source.stem
    device_name = stem.split(".", 1)[0].casefold()
    if (
        not stem
        or stem in {".", ".."}
        or stem.endswith((".", " "))
        or device_name in {"con", "prn", "aux", "nul"}
        or device_name
        in {f"{prefix}{n}" for prefix in ("com", "lpt") for n in range(1, 10)}
        or any(c in '<>:"/\\|?*' or unicodedata.category(c) == "Cc" for c in stem)
    ):
        raise ValueError(f"invalid Trace Streamer source stem: {stem!r}")
    return stem
