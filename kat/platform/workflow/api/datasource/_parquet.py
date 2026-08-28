from __future__ import annotations

from collections.abc import Mapping
from datetime import datetime
from decimal import Decimal
from pathlib import Path
import shutil
from types import TracebackType
from typing import Self

from datafusion import SessionContext
import pyarrow as pa
import pyarrow.dataset as pads
import pyarrow.parquet as pq

from ._schema import Schema, _logical_type
from ._sql import execute_sql
from ._table import Table, table, to_arrow


def write(schema: Schema, *, destination: Path) -> _Writer:
    """Create a strict multi-table Parquet writer at a new destination."""
    if not isinstance(schema, Schema):
        raise TypeError("ds.write schema must be a ds.Schema")
    _require_path(destination, "destination")
    return _Writer(schema, destination)


class _Writer:
    __slots__ = (
        "_schema",
        "_destination",
        "_writers",
        "_failure",
        "_closed",
        "_created",
    )

    def __init__(self, schema: Schema, destination: Path) -> None:
        self._schema = schema
        self._destination = destination
        self._writers: dict[str, pq.ParquetWriter] = {}
        self._failure: BaseException | None = None
        self._closed = False
        self._created = False
        try:
            destination.mkdir()
            self._created = True
            for table_name in schema.tables:
                self._writers[table_name] = pq.ParquetWriter(
                    destination / f"{table_name}.parquet",
                    schema._arrow_schema(table_name),
                )
        except BaseException as error:
            self._failure = error
            close_failure = self._close_all()
            if close_failure is not None:
                _add_note(error, "Datasource writer also failed to close", close_failure)
            self._clean_destination(error)
            raise

    def __enter__(self) -> Self:
        if self._closed:
            raise RuntimeError("Datasource writer is already closed")
        return self

    def __exit__(
        self,
        error_type: type[BaseException] | None,
        error: BaseException | None,
        traceback: TracebackType | None,
    ) -> bool:
        del error_type, traceback
        if self._closed:
            return False

        self._closed = True
        close_failure = self._close_all()
        primary = self._failure or error or close_failure
        if primary is not None:
            if close_failure is not None and close_failure is not primary:
                _add_note(primary, "Datasource writer also failed to close", close_failure)
            self._clean_destination(primary)

        if self._failure is not None:
            raise self._failure
        if error is not None:
            return False
        if close_failure is not None:
            raise close_failure
        return False

    def write(self, table_name: str, /, **columns: list[object | None]) -> None:
        if self._failure is not None:
            raise self._failure
        if self._closed:
            raise RuntimeError("Datasource writer is already closed")

        try:
            if type(table_name) is not str or table_name not in self._writers:
                raise ValueError(f"unknown Datasource table: {table_name!r}")
            declared = tuple(self._schema[table_name])
            if set(columns) != set(declared) or len(columns) != len(declared):
                raise ValueError(
                    f"columns for table {table_name!r} must exactly match {declared!r}"
                )
            for column_name in declared:
                if type(columns[column_name]) is not list:
                    raise TypeError(
                        f"column {column_name!r} for table {table_name!r} must be a list"
                    )
            lengths = {len(columns[column_name]) for column_name in declared}
            if len(lengths) != 1:
                raise ValueError(
                    f"columns for table {table_name!r} must have equal lengths"
                )

            batch = table(schema=self._schema[table_name], columns=columns)
            self._writers[table_name].write_table(to_arrow(batch))
        except BaseException as error:
            self._failure = error
            raise

    def _close_all(self) -> BaseException | None:
        first: BaseException | None = None
        for writer in self._writers.values():
            try:
                writer.close()
            except BaseException as error:
                if first is None:
                    first = error
        return first

    def _clean_destination(self, primary: BaseException) -> None:
        if not self._created:
            return
        try:
            if self._destination.exists():
                shutil.rmtree(self._destination)
        except BaseException as cleanup_error:
            _add_note(
                primary,
                "Datasource writer also failed to clean its destination",
                cleanup_error,
            )


def open(
    schema: Schema,
    *,
    root: Path | None = None,
    tables: Mapping[str, Path] | None = None,
) -> Catalog:
    """Open and metadata-check a live Parquet catalog."""
    if not isinstance(schema, Schema):
        raise TypeError("ds.open schema must be a ds.Schema")
    if (root is None) == (tables is None):
        raise TypeError("ds.open requires exactly one of root or tables")

    if root is not None:
        _require_path(root, "root")
        if not root.is_dir():
            raise ValueError("ds.open root must be an existing directory")
        discovered = {
            path.stem: (path,)
            for path in sorted(root.iterdir(), key=lambda item: item.name)
            if path.is_file() and path.suffix == ".parquet"
        }
        _require_table_set(schema, discovered)
        paths_by_table = discovered
    else:
        if not isinstance(tables, Mapping):
            raise TypeError("ds.open tables must be a Mapping of table names to Paths")
        snapshot = dict(tables)
        _require_table_set(schema, snapshot)
        paths_by_table: dict[str, tuple[Path, ...]] = {}
        for table_name in schema.tables:
            path = snapshot[table_name]
            _require_path(path, f"path for table {table_name!r}")
            paths_by_table[table_name] = _table_paths(table_name, path)

    physical_schemas: dict[str, pa.Schema] = {}
    for table_name in schema.tables:
        paths = paths_by_table[table_name]
        physical = _read_and_validate_parts(table_name, schema, paths)
        physical_schemas[table_name] = physical

    session = SessionContext()
    datasets: list[pads.Dataset] = []
    for table_name in schema.tables:
        dataset = pads.dataset(
            [str(path) for path in paths_by_table[table_name]],
            format="parquet",
            partitioning=None,
            schema=physical_schemas[table_name],
        )
        session.register_table(table_name, dataset)
        datasets.append(dataset)
    return Catalog(session, tuple(datasets))


class Catalog:
    """A reusable, read-only live view over validated Parquet paths."""

    __slots__ = ("_session", "_datasets")

    def __init__(
        self,
        session: SessionContext,
        datasets: tuple[pads.Dataset, ...],
    ) -> None:
        self._session = session
        # Keep PyArrow's providers alive for as long as DataFusion may scan them.
        self._datasets = datasets

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> Table:
        return execute_sql(self._session, sql, params=params)


def _require_path(path: object, name: str) -> None:
    if not isinstance(path, Path):
        raise TypeError(f"{name} must be a pathlib.Path")


def _require_table_set(schema: Schema, actual: Mapping[object, object]) -> None:
    expected = set(schema.tables)
    names = set(actual)
    if names != expected or len(actual) != len(expected):
        raise ValueError(
            "Parquet catalog table set must exactly match Datasource Schema; "
            f"expected {tuple(schema.tables)!r}, got {tuple(actual)!r}"
        )


def _table_paths(table_name: str, path: Path) -> tuple[Path, ...]:
    if path.is_file():
        return (path,)
    if path.is_dir():
        parts = tuple(
            sorted(
                (
                    item
                    for item in path.rglob("*")
                    if item.is_file() and item.suffix == ".parquet"
                ),
                key=lambda item: item.relative_to(path).as_posix(),
            )
        )
        if not parts:
            raise ValueError(
                f"parts directory for table {table_name!r} must contain at least one Parquet file"
            )
        return parts
    raise ValueError(f"path for table {table_name!r} must be a file or directory")


def _read_and_validate_parts(
    table_name: str,
    logical_schema: Schema,
    paths: tuple[Path, ...],
) -> pa.Schema:
    first: pa.Schema | None = None
    for path in paths:
        physical = pq.read_schema(path)
        if first is None:
            first = physical
            _validate_logical_schema(
                table_name,
                logical_schema[table_name],
                physical,
            )
        elif not physical.equals(first, check_metadata=False):
            raise TypeError(
                f"all Parquet parts for table {table_name!r} must have the same physical schema"
            )
    if first is None:
        raise ValueError(f"table {table_name!r} must contain at least one Parquet file")
    return first


def _validate_logical_schema(
    table_name: str,
    logical: Mapping[str, object],
    physical: pa.Schema,
) -> None:
    expected_names = tuple(logical)
    actual_names = tuple(physical.names)
    if actual_names != expected_names:
        raise ValueError(
            f"Parquet table {table_name!r} columns must exactly match order "
            f"{expected_names!r}; got {actual_names!r}"
        )
    for field in physical:
        python_type, nullable = _logical_type(logical[field.name])
        if not nullable and field.nullable:
            raise TypeError(
                f"Parquet column {table_name}.{field.name} is nullable but the "
                "Datasource Schema requires a non-nullable column"
            )
        if not _compatible_type(python_type, field.type):
            raise TypeError(
                f"Parquet column {table_name}.{field.name} type {field.type} is "
                f"not compatible with Datasource type {python_type.__name__}"
            )


def _compatible_type(python_type: type[object], arrow_type: pa.DataType) -> bool:
    if python_type is bool:
        return pa.types.is_boolean(arrow_type)
    if python_type is int:
        return pa.types.is_integer(arrow_type)
    if python_type is float:
        return pa.types.is_float32(arrow_type) or pa.types.is_float64(arrow_type)
    if python_type is str:
        return (
            pa.types.is_string(arrow_type)
            or pa.types.is_large_string(arrow_type)
            or _is_string_view(arrow_type)
        )
    if python_type is bytes:
        return pa.types.is_binary(arrow_type) or pa.types.is_large_binary(arrow_type)
    if python_type is datetime:
        return (
            pa.types.is_timestamp(arrow_type)
            and arrow_type.unit == "ns"
            and arrow_type.tz == "UTC"
        )
    if python_type is Decimal:
        return pa.types.is_decimal128(arrow_type) or pa.types.is_decimal256(arrow_type)
    return False


def _is_string_view(data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, "is_string_view", None)
    return bool(predicate and predicate(data_type))


def _add_note(primary: BaseException, message: str, secondary: BaseException) -> None:
    try:
        primary.add_note(f"{message}: {secondary}")
    except BaseException:
        pass
