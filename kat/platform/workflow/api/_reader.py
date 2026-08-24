from __future__ import annotations

from collections.abc import Callable, Iterator, Mapping, Sequence
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass, field
from inspect import getattr_static
from pathlib import Path
import tempfile
import threading
from typing import cast
from weakref import WeakKeyDictionary

from datafusion import SessionContext
from datafusion.catalog import Catalog, Schema, SchemaProvider, Table
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq

from ._identifiers import valid_table_name


@dataclass(eq=False)
class _ReaderOperation:
    session: SessionContext
    staging: Path
    _allocation_lock: threading.Lock = field(default_factory=threading.Lock)
    _next_file: int = 0

    def allocate(self) -> Path:
        with self._allocation_lock:
            sequence = self._next_file
            self._next_file += 1
        return self.staging / f"{sequence:08d}.parquet"


@dataclass
class _TableResolution:
    lock: threading.Lock = field(default_factory=threading.Lock)
    table: Table | None = None
    error: BaseException | None = None
    complete: bool = False


_ACTIVE_READER_OPERATION: ContextVar[_ReaderOperation | None] = ContextVar(
    "kat_active_reader_operation",
    default=None,
)


class _ReaderSchemaProvider(SchemaProvider):
    def __init__(
        self,
        factories: Mapping[str, Callable[[], pa.RecordBatchReader]],
    ) -> None:
        self._factories = dict(factories)
        self._resolutions: WeakKeyDictionary[
            _ReaderOperation, dict[str, _TableResolution]
        ] = WeakKeyDictionary()
        self._resolutions_lock = threading.Lock()

    # DataFusion 54 的 Rust bridge 会把该成员当作 Sequence 属性读取，
    # 虽然 Python 抽象基类把它定义为方法。
    @property
    def table_names(self) -> tuple[str, ...]:  # type: ignore[override]
        return tuple(sorted(self._factories))

    def table_exist(self, name: str) -> bool:
        return name in self._factories

    def table(self, name: str) -> Table | None:
        factory = self._factories.get(name)
        if factory is None:
            return None
        operation = _ACTIVE_READER_OPERATION.get()
        if operation is None:
            raise RuntimeError(
                "schema_from_readers tables can be opened only during a KAT Source operation"
            )
        resolution = self._resolution(operation, name)
        with resolution.lock:
            if resolution.complete:
                if resolution.error is not None:
                    raise RuntimeError(
                        f"reader factory for table {name!r} failed earlier in this Source operation"
                    ) from resolution.error
                return resolution.table
            try:
                resolution.table = _stage_reader(operation, name, factory)
            except BaseException as error:
                resolution.error = error
                resolution.complete = True
                raise
            resolution.complete = True
            return resolution.table

    def _resolution(
        self,
        operation: _ReaderOperation,
        name: str,
    ) -> _TableResolution:
        with self._resolutions_lock:
            by_name = self._resolutions.setdefault(operation, {})
            return by_name.setdefault(name, _TableResolution())


class _SchemaProviderAdapter(SchemaProvider):
    def __init__(self, provider: object) -> None:
        self._provider = provider

    @property
    def table_names(self) -> tuple[str, ...]:  # type: ignore[override]
        member = getattr(self._provider, "table_names")
        names = member() if callable(member) else member
        if isinstance(names, (str, bytes)) or not isinstance(names, Sequence | set):
            raise TypeError("Source schema provider table_names must be a sequence")
        copied = list(names)
        if any(type(name) is not str or not valid_table_name(name) for name in copied):
            raise ValueError("Source schema provider returned an invalid table name")
        if len(set(copied)) != len(copied):
            raise ValueError("Source schema provider returned duplicate table names")
        return tuple(sorted(copied))

    def table_exist(self, name: str) -> bool:
        return name in self.table_names

    def table(self, name: str) -> Table | None:
        table = getattr(self._provider, "table")(name)
        return cast(Table | None, table)


def schema_from_readers(
    factories: Mapping[str, Callable[[], pa.RecordBatchReader]],
) -> SchemaProvider:
    """Build a lazy DataFusion schema from Arrow RecordBatchReader factories.

    Table discovery never calls a factory. The first access to a table during a
    KAT Source operation consumes that table's reader incrementally into a
    private Parquet staging file. Each factory runs at most once per operation.
    """
    if not isinstance(factories, Mapping):
        raise TypeError("schema_from_readers factories must be a mapping")
    copied = dict(factories)
    for name, factory in copied.items():
        if type(name) is not str or not valid_table_name(name):
            raise ValueError(f"invalid reader table name: {name!r}")
        if not callable(factory):
            raise TypeError(f"reader factory for table {name!r} must be callable")
    return _ReaderSchemaProvider(copied)


def _adapt_schema_provider(provider: object) -> object:
    """返回 DataFusion 54 能够原生接收的 Provider 形态。"""
    if isinstance(provider, _SchemaProviderAdapter):
        return provider
    if isinstance(provider, Schema):
        # 公共 Schema 包装在调用 table() 时会阻塞其 Tokio runtime。返回官方
        # RawSchema 后，DataFusion 无须从 Python 回调重新进入该 runtime。
        return provider._raw_schema
    if isinstance(provider, SchemaProvider):
        return _SchemaProviderAdapter(provider)
    raise TypeError("Source Entry must return a DataFusion schema provider")


def _normalize_schema_provider(
    provider: object,
    *,
    session: SessionContext,
) -> object:
    """把 Source 结果转换为可缓存且可从 catalog 回调安全返回的形态。"""
    if callable(getattr_static(provider, "__datafusion_schema_provider__", None)):
        catalog = Catalog.memory_catalog(session)
        catalog.register_schema("__kat_normalized__", provider)  # type: ignore[arg-type]
        provider = catalog.schema("__kat_normalized__")
    return _adapt_schema_provider(provider)


def _enumerable_schema_provider(
    provider: object,
    *,
    session: SessionContext,
) -> SchemaProvider:
    """在 DataFusion 的 Provider 回调之外提供表发现能力。"""
    if isinstance(provider, SchemaProvider):
        return provider
    catalog = Catalog.memory_catalog(session)
    catalog.register_schema("__kat_source__", provider)  # type: ignore[arg-type]
    return _SchemaProviderAdapter(catalog.schema("__kat_source__"))


@contextmanager
def _reader_source_operation(
    session: SessionContext,
    *,
    staging_parent: Path | None = None,
) -> Iterator[Path]:
    """将一个私有 reader staging 的生命周期绑定到当前 Source operation。"""
    if not isinstance(session, SessionContext):
        raise TypeError("reader Source operation requires a DataFusion SessionContext")
    parent = None if staging_parent is None else str(staging_parent)
    temporary = tempfile.TemporaryDirectory(
        prefix="kat-source-staging-",
        dir=parent,
    )
    staging = Path(temporary.name)
    operation = _ReaderOperation(session=session, staging=staging)
    token = _ACTIVE_READER_OPERATION.set(operation)
    try:
        yield staging
    finally:
        _ACTIVE_READER_OPERATION.reset(token)
        try:
            temporary.cleanup()
        except OSError:
            pass


def _stage_reader(
    operation: _ReaderOperation,
    name: str,
    factory: Callable[[], pa.RecordBatchReader],
) -> Table:
    path = operation.allocate()
    reader: pa.RecordBatchReader | None = None
    try:
        value = factory()
        if not isinstance(value, pa.RecordBatchReader):
            raise TypeError(
                f"reader factory for table {name!r} must return pyarrow.RecordBatchReader"
            )
        reader = value
        schema = reader.schema
        with pq.ParquetWriter(path, schema) as writer:
            for batch in reader:
                writer.write_batch(batch)
        # 该回调在 DataFusion 的 Tokio runtime 中执行。如果在此调用
        # SessionContext.read_parquet，DataFusion 54 会尝试启动嵌套 runtime。
        # 因此通过官方 PyArrow Dataset TableProvider 路径交付已完成的 Parquet。
        return Table(ds.dataset(path, format="parquet", schema=schema))
    except BaseException:
        try:
            path.unlink(missing_ok=True)
        except OSError:
            pass
        raise
    finally:
        if reader is not None:
            try:
                reader.close()
            except Exception:
                pass
