from __future__ import annotations

import hashlib
import logging
import os
from pathlib import Path
import shutil
import stat
import uuid

import kat
import pyarrow as pa
import pyarrow.parquet as pq
from datafusion import SessionContext
from kat._identifiers import valid_output_name


_LOGGER = logging.getLogger(__name__)


class WorkflowOperation:
    """Owns Provider results and capabilities for one Workflow execution."""

    def __init__(
        self,
        session: SessionContext,
        candidate_path: Path,
        datasource_root: Path,
    ) -> None:
        self._session = session
        self._candidate_path = candidate_path
        self._datasource_root = datasource_root
        self._output_root = candidate_path / "outputs"
        self._scratch_root = candidate_path / ".scratch"
        self._operation_token = object()
        self._active = True
        self._queries_open = True
        self._failure: BaseException | None = None
        self._reserved_names: set[str] = set()
        self._tables: dict[str, kat.Table] = {}
        self._executors: list[object] = []
        self._published_tables: set[str] = set()

    def require_active(self) -> None:
        if not self._active:
            raise RuntimeError("Workflow execution lease is no longer active")

    def require_usable(self) -> None:
        self.require_active()
        if self._failure is not None:
            raise RuntimeError(
                "Workflow Context cannot continue after a Provider query failed"
            ) from self._failure

    def require_publishable(self) -> None:
        self.require_active()
        if self._failure is not None:
            raise RuntimeError(
                "Workflow Context cannot publish after a Provider query failed"
            ) from self._failure

    def provider(self, executor: object) -> kat.Provider:
        self.require_usable()
        if not callable(getattr(executor, "execute", None)) or not callable(
            getattr(executor, "close", None)
        ):
            raise TypeError("ctx.provider requires a structural SourceExecutor")
        if all(registered is not executor for registered in self._executors):
            self._executors.append(executor)
        return kat.Provider._create(
            lambda sql, params, name: self._query(executor, sql, params, name)
        )

    @property
    def datasource_root(self) -> Path:
        self.require_usable()
        root = self._datasource_root
        try:
            if root.is_symlink() or _is_junction(root):
                raise OSError("Datasource root must not be a link")
            root.mkdir(parents=True, exist_ok=True)
            resolved = root.resolve(strict=True)
            parent = root.parent.resolve(strict=True)
        except (OSError, RuntimeError):
            _LOGGER.exception("failed to prepare the private Datasource root")
            raise RuntimeError("Datasource root could not be prepared") from None
        if resolved != root or resolved.parent != parent or not resolved.is_dir():
            raise RuntimeError("Datasource root is not a canonical directory")
        return resolved

    @property
    def provider_names(self) -> frozenset[str]:
        return frozenset(self._tables)

    @property
    def output_root(self) -> Path:
        return self._output_root

    def table_facts(self, table: kat.Table) -> tuple[pa.Schema, int]:
        operation, path, row_count = table._runtime_facts()
        if operation is not self._operation_token:
            raise ValueError("Table belongs to a different Workflow execution")
        if self._tables.get(table.name) is not table:
            raise ValueError("Table is not registered in this Workflow execution")
        if path != self._output_root / f"{table.name}.parquet" or not path.exists():
            raise ValueError(f"Table {table.name!r} backing is unavailable")
        return table.schema, row_count

    def mark_published_tables(self, names: set[str]) -> None:
        self._published_tables = set(names)

    def close_executors(self) -> None:
        self._queries_open = False
        executors, self._executors = self._executors, []
        for executor in executors:
            try:
                executor.close()
            except BaseException:
                _LOGGER.warning("Source executor cleanup failed", exc_info=True)

    def cleanup(self, *, success: bool) -> None:
        keep = self._published_tables if success else set()
        for name, table in self._tables.items():
            if name in keep:
                continue
            _, path, _ = table._runtime_facts()
            _remove_best_effort(path, f"unpublished Provider Table {name!r}")
        _remove_best_effort(self._scratch_root, "Workflow query scratch")

    def expire(self) -> None:
        self._queries_open = False
        self._active = False

    def _query(
        self,
        executor: object,
        sql: str,
        params: object | None,
        name: str | None,
    ) -> kat.Table:
        self.require_usable()
        if not self._queries_open:
            raise RuntimeError("Workflow Provider is no longer active")
        try:
            return self._query_once(executor, sql, params, name)
        except BaseException as error:
            if self._failure is None:
                self._failure = error
            raise

    def _query_once(
        self,
        executor: object,
        sql: str,
        params: object | None,
        name: str | None,
    ) -> kat.Table:
        if type(sql) is not str or not sql.strip():
            raise TypeError("Provider.query requires a non-empty SQL string")
        result_name = _query_name(sql, name)
        if result_name in self._reserved_names or self._session.table_exist(result_name):
            raise ValueError(f"query result name is already in use: {result_name!r}")
        self._reserved_names.add(result_name)

        self._prepare_output_root()
        final_path = self._output_root / f"{result_name}.parquet"
        if os.path.lexists(final_path):
            raise ValueError(f"query result backing already exists: {result_name!r}")

        token = uuid.uuid4().hex
        scratch = self._scratch_root / f"{result_name}-{token}"
        partial = self._output_root / f".{result_name}.{token}.partial"
        self._prepare_scratch_root()
        scratch.mkdir()
        try:
            manager = executor.execute(sql, params, scratch=scratch)
            source_failure: BaseException | None = None
            validate_partial_contents = False
            try:
                with manager as source:
                    try:
                        if isinstance(source, pa.RecordBatchReader):
                            _write_reader(source, partial)
                        elif isinstance(source, kat.ParquetSource):
                            _stage_parquet_source(source.path, scratch, partial)
                            validate_partial_contents = True
                        else:
                            raise TypeError(
                                "SourceExecutor.execute must yield a RecordBatchReader or ParquetSource"
                            )
                    except BaseException as error:
                        source_failure = error
                        raise
            except BaseException as error:
                if source_failure is None or error is source_failure:
                    raise
                _LOGGER.warning(
                    "Source executor context cleanup failed after a query failure",
                    exc_info=True,
                )
                raise source_failure from None
            if source_failure is not None:
                raise source_failure
            schema, row_count = _parquet_facts(
                partial,
                validate_contents=validate_partial_contents,
            )
            os.replace(partial, final_path)
            try:
                self._session.register_parquet(
                    result_name,
                    str(final_path),
                    schema=schema,
                )
            except BaseException:
                try:
                    self._session.deregister_table(result_name)
                except BaseException:
                    _LOGGER.warning(
                        "failed to undo a partial Provider relation registration",
                        exc_info=True,
                    )
                raise
            table = kat.Table._create(
                name=result_name,
                schema=schema,
                operation=self._operation_token,
                backing_path=final_path,
                row_count=row_count,
            )
            self._tables[result_name] = table
            return table
        except BaseException:
            _remove_best_effort(partial, f"partial Provider Table {result_name!r}")
            _remove_best_effort(final_path, f"failed Provider Table {result_name!r}")
            raise
        finally:
            _remove_best_effort(scratch, f"query scratch for {result_name!r}")

    def _prepare_output_root(self) -> None:
        if self._output_root.is_symlink() or _is_junction(self._output_root):
            raise OSError("Run Output directory must not be a link")
        self._output_root.mkdir(exist_ok=True)
        if not self._output_root.is_dir():
            raise OSError("Run Output path is not a directory")

    def _prepare_scratch_root(self) -> None:
        if self._scratch_root.is_symlink() or _is_junction(self._scratch_root):
            raise OSError("Workflow query scratch directory must not be a link")
        self._scratch_root.mkdir(exist_ok=True)
        if not self._scratch_root.is_dir():
            raise OSError("Workflow query scratch path is not a directory")


def _query_name(sql: str, supplied: str | None) -> str:
    if supplied is None:
        name = f"q_{hashlib.sha256(sql.encode('utf-8')).hexdigest()}"
    else:
        if type(supplied) is not str:
            raise TypeError("Provider query result name must be a string or None")
        name = supplied
    if not valid_output_name(name):
        raise ValueError(f"invalid query result name: {name!r}")
    return name


def _write_reader(reader: pa.RecordBatchReader, partial: Path) -> None:
    schema = reader.schema
    if not isinstance(schema, pa.Schema) or len(schema) == 0:
        raise ValueError("Source query result must have a non-empty Arrow Schema")
    sink: pa.NativeFile | None = None
    writer: pq.ParquetWriter | None = None
    failed = False
    try:
        sink = pa.OSFile(str(partial), "wb")
        writer = pq.ParquetWriter(sink, schema, compression="zstd")
        for batch in reader:
            if not isinstance(batch, pa.RecordBatch):
                raise TypeError("Source query stream must yield Arrow RecordBatch values")
            writer.write_batch(batch)
        writer.close()
        writer = None
    except BaseException:
        failed = True
        raise
    finally:
        cleanup_error: BaseException | None = None
        if writer is not None:
            try:
                writer.close()
            except BaseException as error:
                if failed:
                    _LOGGER.warning(
                        "failed to close a partial Provider Parquet writer",
                        exc_info=True,
                    )
                else:
                    cleanup_error = error
        if sink is not None:
            try:
                sink.close()
            except BaseException as error:
                if failed or cleanup_error is not None:
                    _LOGGER.warning(
                        "failed to close a partial Provider file sink",
                        exc_info=True,
                    )
                else:
                    cleanup_error = error
        if cleanup_error is not None:
            raise cleanup_error


def _stage_parquet_source(source: Path, scratch: Path, partial: Path) -> None:
    supplied = source
    if supplied.is_symlink() or _is_junction(supplied):
        raise ValueError("ParquetSource must not be a symbolic link or junction")
    try:
        resolved = supplied.resolve(strict=True)
    except (OSError, RuntimeError):
        raise ValueError("ParquetSource path must exist") from None
    _parquet_facts(resolved, reject_links=True)
    scratch_root = scratch.resolve(strict=True)
    if resolved.is_relative_to(scratch_root):
        os.replace(resolved, partial)
    elif resolved.is_file():
        shutil.copy2(resolved, partial, follow_symlinks=False)
    else:
        shutil.copytree(resolved, partial, symlinks=False)


def _parquet_facts(
    path: Path,
    *,
    reject_links: bool = False,
    validate_contents: bool = False,
) -> tuple[pa.Schema, int]:
    files = _parquet_files(path, reject_links=reject_links)
    schema: pa.Schema | None = None
    row_count = 0
    for file in files:
        parquet = pq.ParquetFile(
            file,
            page_checksum_verification=validate_contents,
        )
        try:
            current = parquet.schema_arrow
            if len(current) == 0:
                raise ValueError(
                    "Source query result must have a non-empty Arrow Schema"
                )
            if schema is None:
                schema = current
            elif not schema.equals(current, check_metadata=False):
                raise ValueError(
                    "ParquetSource parts must have one identical Arrow Schema"
                )
            expected_rows = parquet.metadata.num_rows
            if validate_contents and parquet.scan_contents() != expected_rows:
                raise ValueError(
                    "ParquetSource row count does not match its readable contents"
                )
            row_count += expected_rows
        finally:
            parquet.close()
    if schema is None:
        raise ValueError("ParquetSource dataset must contain a Parquet part")
    return schema, row_count


def _parquet_files(path: Path, *, reject_links: bool) -> list[Path]:
    if reject_links:
        _reject_link_or_hardlink(path)
    if path.is_file():
        return [path]
    if not path.is_dir():
        raise ValueError("ParquetSource must be a file or single-table directory")
    files: list[Path] = []
    pending = [path]
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as entries:
            children = sorted(entries, key=lambda entry: entry.name)
        for entry in children:
            child = Path(entry.path)
            if reject_links:
                _reject_link_or_hardlink(child)
            if entry.is_dir(follow_symlinks=False):
                pending.append(child)
            elif (
                entry.is_file(follow_symlinks=False)
                and child.suffix.lower() == ".parquet"
            ):
                files.append(child)
            else:
                raise ValueError(
                    "ParquetSource directory may contain only Parquet parts"
                )
    if not files:
        raise ValueError("ParquetSource directory must contain a Parquet part")
    return sorted(files, key=lambda item: item.as_posix())


def _reject_link_or_hardlink(path: Path) -> None:
    if path.is_symlink() or _is_junction(path):
        raise ValueError("ParquetSource must not contain links")
    metadata = path.stat(follow_symlinks=False)
    if stat.S_ISREG(metadata.st_mode) and metadata.st_nlink > 1:
        raise ValueError("ParquetSource must not contain hard links")


def _is_junction(path: Path) -> bool:
    checker = getattr(path, "is_junction", None)
    return bool(checker()) if checker is not None else False


def _remove_best_effort(path: Path, label: str) -> None:
    try:
        if _is_junction(path):
            path.rmdir()
        elif path.is_symlink():
            path.unlink(missing_ok=True)
        elif path.is_dir():
            shutil.rmtree(path)
        elif os.path.lexists(path):
            path.unlink(missing_ok=True)
    except (OSError, RuntimeError):
        _LOGGER.warning("failed to remove private %s", label, exc_info=True)
