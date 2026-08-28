from __future__ import annotations

from collections.abc import Iterator, Mapping, Sequence
from contextlib import ExitStack
from pathlib import Path
from types import TracebackType
from urllib.parse import quote
import warnings

from adbc_driver_postgresql import StatementOptions, dbapi
import kat
import pyarrow as pa


_BATCH_SIZE_HINT_BYTES = 1_048_576


def provider(
    ctx: kat.Context,
    *,
    profile: str,
    database: str,
) -> kat.Provider:
    """Create a Provider for one Database selected through a libpq service."""
    return ctx.provider(PostgreSQLExecutor(profile=profile, database=database))


class PostgreSQLExecutor:
    """PACK-owned ADBC executor that isolates every query in one connection."""

    def __init__(self, *, profile: str, database: str) -> None:
        self._profile = _connection_name("profile", profile)
        self._database = _connection_name("database", database)
        self._closed = False

    def execute(
        self,
        sql: str,
        params: object | None,
        *,
        scratch: Path,
    ) -> _PostgreSQLQuery:
        del scratch
        return _PostgreSQLQuery(self, sql, params)

    def close(self) -> None:
        self._closed = True


class _PostgreSQLQuery:
    def __init__(
        self,
        executor: PostgreSQLExecutor,
        sql: str,
        params: object | None,
    ) -> None:
        self._executor = executor
        self._sql = sql
        self._params = params
        self._stack: ExitStack | None = None
        self._entered = False

    def __enter__(self) -> pa.RecordBatchReader:
        if self._entered:
            raise RuntimeError("PostgreSQL query context cannot be entered twice")
        self._entered = True
        if self._executor._closed:
            raise RuntimeError("PostgreSQLExecutor is closed")
        params = _positional_params(self._params)
        stack = ExitStack()
        try:
            connection = dbapi.connect(
                _connection_uri(
                    self._executor._profile,
                    self._executor._database,
                ),
                autocommit=False,
            )
            stack.callback(connection.close)
            stack.callback(connection.rollback)

            setup = connection.cursor()
            try:
                setup.execute("SET TRANSACTION READ ONLY")
            finally:
                setup.close()

            query = connection.cursor()
            stack.callback(query.close)
            query.adbc_statement.set_options(
                **{
                    StatementOptions.BATCH_SIZE_HINT_BYTES.value: str(
                        _BATCH_SIZE_HINT_BYTES
                    )
                }
            )
            query.execute(self._sql, params)
            source = query.fetch_record_batch()
            stack.callback(source.close)
            if len(source.schema) == 0:
                raise _PostgreSQLQueryError(
                    "PostgreSQL query must return at least one column"
                )
            reader = pa.RecordBatchReader.from_batches(
                source.schema,
                _safe_batches(source),
            )
            stack.callback(reader.close)
        except _PostgreSQLQueryError:
            _discard_stack(stack)
            raise
        except BaseException as error:
            _discard_stack(stack)
            if not isinstance(error, Exception):
                raise
            raise _PostgreSQLQueryError("PostgreSQL query failed") from None
        self._stack = stack
        return reader

    def __exit__(
        self,
        error_type: type[BaseException] | None,
        error: BaseException | None,
        traceback: TracebackType | None,
    ) -> bool:
        del error_type, traceback
        stack = self._stack
        self._stack = None
        if stack is None:
            return False
        try:
            _close_stack(stack)
        except Exception:
            if error is None:
                raise _PostgreSQLQueryError(
                    "PostgreSQL query cleanup failed"
                ) from None
        return False


class _PostgreSQLQueryError(RuntimeError):
    pass


def _connection_name(field: str, value: object) -> str:
    if type(value) is not str or not value or "\0" in value:
        raise TypeError(f"PostgreSQL {field} must be a non-empty string without NUL")
    return value


def _connection_uri(profile: str, database: str) -> str:
    return (
        f"postgresql:///{quote(database, safe='')}"
        f"?service={quote(profile, safe='')}"
    )


def _positional_params(params: object | None) -> tuple[object, ...] | None:
    if params is None:
        return None
    if (
        isinstance(params, Mapping)
        or isinstance(params, (str, bytes, bytearray, memoryview))
        or isinstance(params, (pa.Array, pa.ChunkedArray, pa.RecordBatch, pa.Table))
        or isinstance(params, pa.RecordBatchReader)
        or not isinstance(params, Sequence)
    ):
        raise TypeError("PostgreSQL query params must be one single positional sequence")
    values = tuple(params)
    for value in values:
        if (
            isinstance(value, Mapping)
            or isinstance(value, (pa.Array, pa.ChunkedArray, pa.RecordBatch, pa.Table))
            or isinstance(value, pa.RecordBatchReader)
            or (
                isinstance(value, Sequence)
                and not isinstance(value, (str, bytes, bytearray, memoryview))
            )
        ):
            raise TypeError(
                "PostgreSQL query params must be one single positional sequence"
            )
    return values


def _safe_batches(source: pa.RecordBatchReader) -> Iterator[pa.RecordBatch]:
    try:
        yield from source
    except _PostgreSQLQueryError:
        raise
    except Exception:
        raise _PostgreSQLQueryError("PostgreSQL result stream failed") from None


def _close_stack(stack: ExitStack) -> None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", ResourceWarning)
        stack.close()


def _discard_stack(stack: ExitStack) -> None:
    try:
        _close_stack(stack)
    except BaseException:
        pass
