from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
import shutil
from types import MappingProxyType

import pyarrow as pa
import pyarrow.dataset as pads
import pyarrow.parquet as pq

from .._identifiers import valid_table_name
from ._table import Table


def write(tables: Mapping[str, Table], *, destination: Path) -> None:
    """Synchronously write a named Table mapping to a new flat directory."""
    _require_path(destination, "destination")
    if not isinstance(tables, Mapping):
        raise TypeError("ds.write tables must be a Mapping of names to ds.Table values")

    snapshot = dict(tables.items())
    if not snapshot:
        raise ValueError("ds.write tables must not be empty")

    arrow_tables: dict[str, pa.Table] = {}
    for table_name, value in snapshot.items():
        _require_table_name(table_name)
        if not isinstance(value, Table):
            raise TypeError(f"table {table_name!r} must be a ds.Table")
        arrow_tables[table_name] = value.to_arrow()

    created = False
    try:
        destination.mkdir()
        created = True
        for table_name, arrow_table in arrow_tables.items():
            pq.write_table(arrow_table, destination / f"{table_name}.parquet")
    except BaseException as error:
        if created:
            try:
                shutil.rmtree(destination)
            except BaseException as cleanup_error:
                _add_note(
                    error,
                    "Datasource write also failed to clean its destination",
                    cleanup_error,
                )
        raise


def open(
    *,
    root: Path | None = None,
    tables: Mapping[str, Path] | None = None,
) -> Catalog:
    """Open a schema-less, metadata-validated live Parquet catalog."""
    if (root is None) == (tables is None):
        raise TypeError("ds.open requires exactly one of root or tables")

    if root is not None:
        _require_path(root, "root")
        if not root.is_dir():
            raise ValueError("ds.open root must be an existing directory")
        paths_by_table: dict[str, tuple[Path, ...]] = {}
        for path in sorted(root.iterdir(), key=lambda item: item.name):
            if not path.is_file() or path.suffix != ".parquet":
                continue
            table_name = _require_table_name(path.stem)
            paths_by_table[table_name] = (path.resolve(),)
    else:
        if not isinstance(tables, Mapping):
            raise TypeError("ds.open tables must be a Mapping of table names to Paths")
        snapshot = dict(tables.items())
        paths_by_table = {}
        for table_name, path in snapshot.items():
            _require_table_name(table_name)
            _require_path(path, f"path for table {table_name!r}")
            paths_by_table[table_name] = _table_paths(table_name, path)

    if not paths_by_table:
        raise ValueError("a Parquet catalog must contain at least one relation")

    relations = {
        table_name: _open_relation(table_name, paths)
        for table_name, paths in paths_by_table.items()
    }
    return Catalog(relations)


@dataclass(frozen=True, slots=True)
class _Relation:
    paths: tuple[Path, ...]
    schema: pa.Schema

    def dataset(self) -> pads.Dataset:
        return pads.dataset(
            [str(path) for path in self.paths],
            format="parquet",
            partitioning=None,
            schema=self.schema,
        )


class Catalog:
    """A reusable, read-only live view over validated Parquet paths."""

    __slots__ = ("__relations", "__tables")

    def __init__(self, relations: Mapping[str, _Relation]) -> None:
        snapshot = dict(relations.items())
        object.__setattr__(
            self,
            "_Catalog__relations",
            MappingProxyType(snapshot),
        )
        object.__setattr__(
            self,
            "_Catalog__tables",
            tuple(sorted(snapshot)),
        )

    @property
    def tables(self) -> tuple[str, ...]:
        return self.__tables

    def _relation_items(self) -> tuple[tuple[str, _Relation], ...]:
        return tuple(self.__relations.items())


def _open_relation(table_name: str, paths: tuple[Path, ...]) -> _Relation:
    physical = _read_and_validate_parts(table_name, paths)
    relation = _Relation(paths, physical)
    # Constructing the Dataset checks that PyArrow can represent this physical
    # Parquet relation without reading its data rows. The short-lived DataFusion
    # Session is deliberately deferred to DataFusionProvider.query().
    relation.dataset()
    return relation


def _require_path(path: object, name: str) -> None:
    if not isinstance(path, Path):
        raise TypeError(f"{name} must be a pathlib.Path")


def _require_table_name(name: object) -> str:
    if type(name) is not str or not valid_table_name(name):
        raise ValueError(f"invalid Datasource table name: {name!r}")
    return name


def _table_paths(table_name: str, path: Path) -> tuple[Path, ...]:
    if path.is_file():
        return (path.resolve(),)
    if path.is_dir():
        parts = tuple(
            item.resolve()
            for item in sorted(
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
    paths: tuple[Path, ...],
) -> pa.Schema:
    first: pa.Schema | None = None
    for path in paths:
        physical = pq.read_schema(path)
        if first is None:
            first = physical
        elif not physical.equals(first, check_metadata=False):
            raise TypeError(
                f"all Parquet parts for table {table_name!r} must have the same physical schema"
            )
    if first is None:
        raise ValueError(f"table {table_name!r} must contain at least one Parquet file")
    return first


def _add_note(primary: BaseException, message: str, secondary: BaseException) -> None:
    try:
        primary.add_note(f"{message}: {secondary}")
    except BaseException:
        pass
