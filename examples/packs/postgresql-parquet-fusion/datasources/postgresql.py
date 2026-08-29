from __future__ import annotations

from collections.abc import Mapping, Sequence
from contextlib import ExitStack
from urllib.parse import quote
import warnings

from adbc_driver_postgresql import StatementOptions, dbapi
from kat import datasource as ds
import pyarrow as pa
import pyarrow.compute as pc


_BATCH_SIZE_HINT_BYTES = 1_048_576
_NUMERIC_TYPE = pa.decimal128(38, 18)
_POSTGRESQL_TYPE_NAME = b"ADBC:postgresql:typname"


class PostgreSQLProvider:
    """PACK 自有、按 query 选择 Database 的只读 PostgreSQL Provider。"""

    def __init__(self, *, service: str) -> None:
        self._service = _connection_name("service", service)

    def query(
        self,
        sql: str,
        *,
        database: str,
        params: object | None = None,
    ) -> ds.Table:
        """在远端完整执行 SQL，并返回与连接资源脱离的 eager Table。"""
        if type(sql) is not str or not sql.strip():
            raise TypeError("PostgreSQL SQL must be a non-empty string")
        selected_database = _connection_name("database", database)
        positional_params = _positional_params(params)
        resources = ExitStack()

        try:
            connection = dbapi.connect(
                _connection_uri(self._service, selected_database),
                autocommit=False,
            )
            resources.callback(connection.close)
            resources.callback(connection.rollback)

            setup = connection.cursor()
            try:
                setup.execute("SET TRANSACTION READ ONLY")
            finally:
                setup.close()

            query = connection.cursor()
            resources.callback(query.close)
            query.adbc_statement.set_options(
                **{
                    StatementOptions.BATCH_SIZE_HINT_BYTES.value: str(
                        _BATCH_SIZE_HINT_BYTES
                    )
                }
            )
            query.execute(sql, positional_params)
            reader = query.fetch_record_batch()
            resources.callback(reader.close)
            arrow_table = reader.read_all()
            if arrow_table.num_columns == 0:
                raise _UnsupportedResult(
                    "PostgreSQL query must return at least one column"
                )
            normalized = _normalize_result(arrow_table)
        except _UnsupportedResult as error:
            _discard_resources(resources)
            raise RuntimeError(str(error)) from None
        except BaseException as error:
            _discard_resources(resources)
            if not isinstance(error, Exception):
                raise
            raise RuntimeError("PostgreSQL query failed") from None

        try:
            _close_resources(resources)
        except BaseException as error:
            if not isinstance(error, Exception):
                raise
            raise RuntimeError("PostgreSQL query cleanup failed") from None

        try:
            return ds.from_arrow(normalized)
        except Exception:
            raise RuntimeError(
                "PostgreSQL query returned a type outside the KAT Table contract"
            ) from None


class _UnsupportedResult(RuntimeError):
    pass


def _connection_name(field: str, value: object) -> str:
    if type(value) is not str or not value or "\0" in value:
        raise TypeError(
            f"PostgreSQL {field} must be a non-empty string without NUL"
        )
    return value


def _connection_uri(service: str, database: str) -> str:
    return (
        f"postgresql:///{quote(database, safe='')}"
        f"?service={quote(service, safe='')}"
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
        raise TypeError(
            "PostgreSQL query params must be one single positional sequence"
        )
    values = tuple(params)
    for value in values:
        if (
            isinstance(value, Mapping)
            or isinstance(
                value,
                (pa.Array, pa.ChunkedArray, pa.RecordBatch, pa.Table),
            )
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


def _normalize_result(table: pa.Table) -> pa.Table:
    columns: list[pa.ChunkedArray] = []
    fields: list[pa.Field] = []

    for index, field in enumerate(table.schema):
        column = table.column(index)
        if _postgresql_type_name(field) == "numeric":
            try:
                normalized = pc.cast(
                    _extension_storage(column),
                    _NUMERIC_TYPE,
                    safe=True,
                )
            except Exception:
                raise _UnsupportedResult(
                    "PostgreSQL NUMERIC must fit decimal128(38, 18) exactly"
                ) from None
            fields.append(
                pa.field(field.name, _NUMERIC_TYPE, nullable=field.nullable)
            )
            columns.append(normalized)
            continue

        if pa.types.is_timestamp(field.type):
            if field.type.tz is None:
                raise _UnsupportedResult(
                    "PostgreSQL TIMESTAMP WITHOUT TIME ZONE requires an "
                    "explicit SQL conversion"
                )
            try:
                normalized = pc.cast(
                    column,
                    pa.timestamp("ns", tz="UTC"),
                    safe=True,
                )
            except Exception:
                raise _UnsupportedResult(
                    "PostgreSQL timestamp cannot be represented as UTC nanoseconds"
                ) from None
            fields.append(
                pa.field(
                    field.name,
                    pa.timestamp("ns", tz="UTC"),
                    nullable=field.nullable,
                )
            )
            columns.append(normalized)
            continue

        fields.append(
            pa.field(field.name, field.type, nullable=field.nullable)
        )
        columns.append(column)

    return pa.Table.from_arrays(columns, schema=pa.schema(fields))


def _postgresql_type_name(field: pa.Field) -> str | None:
    metadata = field.metadata or {}
    encoded = metadata.get(_POSTGRESQL_TYPE_NAME)
    if encoded is not None:
        try:
            return encoded.decode("utf-8").lower()
        except UnicodeDecodeError:
            return None

    data_type = field.type
    if isinstance(data_type, pa.BaseExtensionType):
        type_name = getattr(data_type, "type_name", None)
        vendor_name = getattr(data_type, "vendor_name", None)
        if type_name is not None and vendor_name == "PostgreSQL":
            return str(type_name).lower()
    return None


def _extension_storage(column: pa.ChunkedArray) -> pa.ChunkedArray:
    if not isinstance(column.type, pa.BaseExtensionType):
        return column
    return pa.chunked_array(
        [chunk.storage for chunk in column.chunks],
        type=column.type.storage_type,
    )


def _close_resources(resources: ExitStack) -> None:
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", ResourceWarning)
        resources.close()


def _discard_resources(resources: ExitStack) -> None:
    try:
        _close_resources(resources)
    except BaseException:
        pass
