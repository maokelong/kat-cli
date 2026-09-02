from __future__ import annotations

import ctypes
import errno
import os
import queue
import shutil
import tempfile
import threading
from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum, auto
from pathlib import Path

import pyarrow as pa

from ._parquet import _require_path
from ._parquet_writer import _ParquetRelationWriter
from ._schema import Schema, _logical_arrow_schema, _normalize_logical_row


_MAX_BATCH_ROWS = 8_192
_MAX_BATCH_BYTES = 64 * 1024 * 1024
_QUEUE_CAPACITY = 1
_QUEUE_WAIT_SECONDS = 0.05


class _State(Enum):
    NEW = auto()
    ACTIVE = auto()
    DRAINING = auto()
    FAILED = auto()
    COMMITTED = auto()
    ABORTED = auto()


@dataclass(frozen=True, slots=True)
class _Batch:
    relation_name: str
    rows: tuple[tuple[object | None, ...], ...]


@dataclass(slots=True)
class _Buffer:
    rows: list[tuple[object | None, ...]]
    estimated_bytes: int = 0


class _RelationSink:
    __slots__ = ("__relation_name", "__transaction")

    def __init__(
        self,
        transaction: _WriteTransaction,
        relation_name: str,
    ) -> None:
        object.__setattr__(self, "_RelationSink__transaction", transaction)
        object.__setattr__(self, "_RelationSink__relation_name", relation_name)

    def append(self, **row_values: object | None) -> None:
        self.__transaction._append(self.__relation_name, row_values)

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Datasource write relation handles are immutable")


class _WriteTransaction:
    def __init__(self, schema: Schema, destination: Path) -> None:
        self._schema = schema
        self._destination = destination
        self._owner_thread: int | None = None
        self._state = _State.NEW
        self._logical_schemas = {
            table_name: schema[table_name] for table_name in schema.tables
        }
        self._arrow_schemas = {
            table_name: _logical_arrow_schema(columns)
            for table_name, columns in self._logical_schemas.items()
        }
        self._buffers = {
            table_name: _Buffer([]) for table_name in schema.tables
        }
        self._relations = {
            table_name: _RelationSink(self, table_name) for table_name in schema.tables
        }
        self._queue: queue.Queue[_Batch] = queue.Queue(maxsize=_QUEUE_CAPACITY)
        self._worker_ready = threading.Event()
        self._failure_lock = threading.Lock()
        self._worker_failure: BaseException | None = None
        self._staging: Path | None = None
        self._worker: threading.Thread | None = None

    def __enter__(self) -> _WriteTransaction:
        if self._state is not _State.NEW:
            raise RuntimeError("a Datasource write is a one-shot context manager")
        self._owner_thread = threading.get_ident()

        try:
            _validate_destination(self._destination)
            self._staging = Path(
                tempfile.mkdtemp(
                    prefix=".kat-write-",
                    dir=self._destination.parent,
                )
            )
            self._worker = threading.Thread(
                target=self._run_worker,
                name="kat-parquet-writer",
            )
            self._worker.start()
            while not self._worker_ready.wait(_QUEUE_WAIT_SECONDS):
                pass
            self._raise_worker_failure()
        except BaseException as error:
            self._cancel_and_join(error)
            self._cleanup_staging(error)
            self._state = _State.ABORTED
            raise

        self._state = _State.ACTIVE
        return self

    def __exit__(
        self,
        _exc_type: type[BaseException] | None,
        error: BaseException | None,
        _traceback: object | None,
    ) -> bool:
        self._require_owner()
        if self._state is not _State.ACTIVE:
            raise RuntimeError("Datasource write context is not active")

        if error is not None:
            self._state = _State.FAILED
            self._cancel_and_join(error)
            self._cleanup_staging(error)
            self._state = _State.ABORTED
            return False

        try:
            self._finish()
        except BaseException as finish_error:
            if self._state is not _State.COMMITTED:
                self._cancel_and_join(finish_error)
                self._cleanup_staging(finish_error)
                self._state = _State.ABORTED
            raise
        return False

    def __getitem__(self, relation_name: str) -> _RelationSink:
        self._require_owner()
        self._require_active()
        self._raise_worker_failure()
        try:
            return self._relations[relation_name]
        except (KeyError, TypeError):
            raise KeyError(relation_name) from None

    def _append(
        self,
        relation_name: str,
        row_values: Mapping[str, object | None],
    ) -> None:
        self._require_owner()
        self._require_active()
        self._raise_worker_failure()

        normalized = _normalize_logical_row(
            self._logical_schemas[relation_name], row_values
        )
        estimated_bytes = _estimate_row_bytes(normalized)
        buffer = self._buffers[relation_name]
        buffer.rows.append(normalized)
        buffer.estimated_bytes += estimated_bytes
        if (
            len(buffer.rows) >= _MAX_BATCH_ROWS
            or buffer.estimated_bytes >= _MAX_BATCH_BYTES
        ):
            self._enqueue_buffer(relation_name)

    def _finish(self) -> None:
        self._state = _State.DRAINING
        self._raise_worker_failure()
        for relation_name in self._schema.tables:
            self._enqueue_buffer(relation_name)
        self._queue.shutdown()
        self._join_worker()
        self._raise_worker_failure()

        staging = self._require_staging()
        actual_files = tuple(sorted(path.name for path in staging.iterdir()))
        expected_files = tuple(
            sorted(f"{relation_name}.parquet" for relation_name in self._schema.tables)
        )
        if actual_files != expected_files:
            raise RuntimeError("Datasource write produced an incomplete relation set")
        self._publish(staging)

    def _enqueue_buffer(self, relation_name: str) -> None:
        buffer = self._buffers[relation_name]
        if not buffer.rows:
            return
        rows = tuple(buffer.rows)
        buffer.rows = []
        buffer.estimated_bytes = 0
        self._enqueue(_Batch(relation_name, rows))

    def _enqueue(self, item: _Batch) -> None:
        while True:
            self._raise_worker_failure()
            try:
                self._queue.put(item, timeout=_QUEUE_WAIT_SECONDS)
                return
            except queue.Full:
                continue
            except queue.ShutDown:
                self._raise_worker_failure()
                raise RuntimeError("Datasource writer queue stopped unexpectedly")

    def _run_worker(self) -> None:
        writers: dict[str, _ParquetRelationWriter] = {}
        try:
            staging = self._require_staging()
            for relation_name in self._schema.tables:
                writers[relation_name] = _ParquetRelationWriter(
                    staging / f"{relation_name}.parquet",
                    self._arrow_schemas[relation_name],
                )
            self._worker_ready.set()

            while True:
                try:
                    item = self._queue.get()
                except queue.ShutDown:
                    break
                writers[item.relation_name].write_rows(item.rows)
        except BaseException as error:
            self._record_worker_failure(error)
        finally:
            for relation_name, writer in writers.items():
                try:
                    writer.close()
                except BaseException as close_error:
                    self._record_worker_failure(
                        close_error,
                        context=f"closing Parquet relation {relation_name!r}",
                    )
            self._worker_ready.set()

    def _record_worker_failure(
        self,
        error: BaseException,
        *,
        context: str | None = None,
    ) -> None:
        first_failure = False
        with self._failure_lock:
            if self._worker_failure is None:
                self._worker_failure = error
                first_failure = True
            elif error is not self._worker_failure:
                _add_note(
                    self._worker_failure,
                    context or "Datasource write also failed",
                    error,
                )
        if first_failure:
            self._queue.shutdown(immediate=True)

    def _raise_worker_failure(self) -> None:
        with self._failure_lock:
            error = self._worker_failure
        if error is not None:
            raise error

    def _cancel_and_join(self, primary: BaseException) -> None:
        self._queue.shutdown(immediate=True)
        worker = self._worker
        if worker is not None and worker.is_alive():
            while worker.is_alive():
                try:
                    worker.join(_QUEUE_WAIT_SECONDS)
                except BaseException as join_error:
                    _add_note(
                        primary,
                        "Datasource write was also interrupted while "
                        "waiting for its writer",
                        join_error,
                    )
        with self._failure_lock:
            worker_error = self._worker_failure
        if worker_error is not None and worker_error is not primary:
            _add_note(
                primary,
                "Datasource write background writer also failed",
                worker_error,
            )

    def _join_worker(self) -> None:
        worker = self._worker
        if worker is None:
            raise RuntimeError("Datasource writer was not started")
        worker.join()

    def _cleanup_staging(self, primary: BaseException) -> None:
        staging = self._staging
        if staging is None or not staging.exists():
            return
        try:
            shutil.rmtree(staging)
        except BaseException as cleanup_error:
            _add_note(
                primary,
                "Datasource write also failed to clean its staging directory",
                cleanup_error,
            )

    def _publish(self, staging: Path) -> None:
        try:
            _rename_no_replace(staging, self._destination)
        except BaseException:
            if not staging.exists() and self._destination.is_dir():
                self._staging = None
                self._state = _State.COMMITTED
            raise
        self._staging = None
        self._state = _State.COMMITTED

    def _require_owner(self) -> None:
        if (
            self._owner_thread is not None
            and threading.get_ident() != self._owner_thread
        ):
            raise RuntimeError(
                "a Datasource write may only be used by its owner thread"
            )

    def _require_active(self) -> None:
        if self._state is not _State.ACTIVE:
            raise RuntimeError(
                "Datasource write relation handles are only valid inside its "
                "active context"
            )

    def _require_staging(self) -> Path:
        if self._staging is None:
            raise RuntimeError("Datasource write staging is unavailable")
        return self._staging


def write(schema: Schema, *, destination: Path) -> _WriteTransaction:
    """Create a one-shot streaming Datasource write context."""
    if not isinstance(schema, Schema):
        raise TypeError("dp.write schema must be a dp.Schema")
    _require_path(destination, "destination")
    destination = destination.parent.resolve(strict=False) / destination.name
    _validate_destination(destination)
    return _WriteTransaction(schema, destination)


def _estimate_row_bytes(row: tuple[object | None, ...]) -> int:
    size = 0
    for value in row:
        if value is None:
            size += 1
        elif type(value) is str:
            size += len(value.encode("utf-8"))
        elif type(value) is bytes:
            size += len(value)
        elif type(value) is bool:
            size += 1
        elif type(value) is int or type(value) is float:
            size += 8
        else:
            size += 16
    return size


def _validate_destination(destination: Path) -> None:
    if os.path.lexists(destination):
        raise FileExistsError(errno.EEXIST, "destination already exists", destination)
    parent = destination.parent
    if not parent.is_dir():
        raise ValueError(
            "dp.write destination parent must be an existing directory"
        )


def _add_note(primary: BaseException, message: str, secondary: BaseException) -> None:
    try:
        primary.add_note(f"{message}: {secondary}")
        for detail in getattr(secondary, "__notes__", ()):
            primary.add_note(f"{message} detail: {detail}")
    except BaseException:
        pass


def _rename_no_replace(source: Path, destination: Path) -> None:
    if os.name == "nt":
        os.rename(source, destination)
        return

    if os.name == "posix" and hasattr(os, "uname") and os.uname().sysname == "Linux":
        renameat2 = ctypes.CDLL(None, use_errno=True).renameat2
        renameat2.argtypes = [
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        renameat2.restype = ctypes.c_int
        if renameat2(
            -100,
            os.fsencode(source),
            -100,
            os.fsencode(destination),
            1,
        ) != 0:
            error_number = ctypes.get_errno()
            raise OSError(error_number, os.strerror(error_number), destination)
        return

    raise OSError(
        errno.ENOTSUP,
        "atomic no-replace directory publication is unsupported on this platform",
        destination,
    )
