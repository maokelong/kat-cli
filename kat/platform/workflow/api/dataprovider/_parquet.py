from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
import os
from pathlib import Path
import stat
from types import MappingProxyType

import pyarrow as pa
import pyarrow.dataset as pads
import pyarrow.parquet as pq

from .._identifiers import valid_table_name


_CATALOG_CONSTRUCTION_TOKEN = object()


def open(
    *,
    root: Path | None = None,
    tables: Mapping[str, Path] | None = None,
) -> Catalog:
    """Open a schema-less, metadata-validated live Parquet catalog."""
    if (root is None) == (tables is None):
        raise TypeError("dp.open requires exactly one of root or tables")

    if root is not None:
        _require_path(root, "root")
        if _is_link_or_reparse(root) or not root.is_dir():
            raise ValueError("dp.open root must be an existing ordinary directory")
        resolved_root = root.resolve(strict=True)
        paths_by_table: dict[str, tuple[Path, ...]] = {}
        for path in sorted(root.iterdir(), key=lambda item: item.name):
            if path.suffix != ".parquet":
                continue
            if _is_link_or_reparse(path):
                raise ValueError(
                    "dp.open root relations must be direct ordinary Parquet files"
                )
            if not path.is_file():
                continue
            resolved = path.resolve(strict=True)
            if resolved.parent != resolved_root:
                raise ValueError(
                    "dp.open root relations must be direct ordinary Parquet files"
                )
            table_name = _require_table_name(path.stem)
            paths_by_table[table_name] = (resolved,)
    else:
        if not isinstance(tables, Mapping):
            raise TypeError("dp.open tables must be a Mapping of table names to Paths")
        snapshot = dict(tables.items())
        paths_by_table = {}
        for table_name, path in snapshot.items():
            _require_table_name(table_name)
            _require_path(path, f"path for table {table_name!r}")
            paths_by_table[table_name] = _table_paths(table_name, path)

    if root is not None and not paths_by_table:
        raise ValueError("a Parquet catalog must contain at least one relation")

    relations = {
        table_name: _open_relation(table_name, paths)
        for table_name, paths in paths_by_table.items()
    }
    return Catalog(relations, _token=_CATALOG_CONSTRUCTION_TOKEN)


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

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Catalog cannot be subclassed")

    def __init__(
        self,
        relations: Mapping[str, _Relation],
        *,
        _token: object | None = None,
    ) -> None:
        if _token is not _CATALOG_CONSTRUCTION_TOKEN:
            raise TypeError("dp.Catalog values are created by dp.open")
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

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Catalog attributes are immutable")


def _open_relation(table_name: str, paths: tuple[Path, ...]) -> _Relation:
    physical = _read_and_validate_parts(table_name, paths)
    for field in physical:
        if not _is_catalog_scan_type(field.type):
            raise TypeError(
                f"Parquet table {table_name!r} column {field.name!r} has "
                f"unsupported Catalog Arrow type {field.type}"
            )
    relation = _Relation(paths, physical)
    # 构造 Dataset 只验证 PyArrow 可按已准入 Schema 描述这些路径，不创建
    # DataFusion Session，也不读取数据行。
    relation.dataset()
    return relation


def _is_catalog_scan_type(data_type: pa.DataType) -> bool:
    if isinstance(data_type, pa.BaseExtensionType):
        return False
    if (
        pa.types.is_null(data_type)
        or pa.types.is_boolean(data_type)
        or pa.types.is_integer(data_type)
        or pa.types.is_floating(data_type)
        or pa.types.is_string(data_type)
        or pa.types.is_large_string(data_type)
        or _matches_arrow_type("is_string_view", data_type)
        or pa.types.is_binary(data_type)
        or pa.types.is_large_binary(data_type)
        or _matches_arrow_type("is_binary_view", data_type)
        or pa.types.is_fixed_size_binary(data_type)
        or pa.types.is_date(data_type)
        or pa.types.is_time(data_type)
        or pa.types.is_timestamp(data_type)
        or pa.types.is_duration(data_type)
        or pa.types.is_decimal(data_type)
    ):
        return True
    if (
        pa.types.is_list(data_type)
        or pa.types.is_large_list(data_type)
        or pa.types.is_fixed_size_list(data_type)
    ):
        return _is_catalog_scan_type(data_type.value_type)
    if pa.types.is_struct(data_type):
        return all(
            type(field.name) is str
            and bool(field.name)
            and _is_catalog_scan_type(field.type)
            for field in data_type
        )
    if pa.types.is_map(data_type):
        return _is_catalog_scan_type(data_type.key_type) and _is_catalog_scan_type(
            data_type.item_type
        )
    if pa.types.is_dictionary(data_type):
        return _is_catalog_scan_type(
            data_type.index_type
        ) and _is_catalog_scan_type(data_type.value_type)
    return False


def _matches_arrow_type(predicate_name: str, data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, predicate_name, None)
    return bool(predicate and predicate(data_type))


def _require_path(path: object, name: str) -> None:
    if not isinstance(path, Path):
        raise TypeError(f"{name} must be a pathlib.Path")


def _is_link_or_reparse(path: Path) -> bool:
    if path.is_symlink():
        return True
    junction = getattr(path, "is_junction", None)
    if junction is not None and junction():
        return True
    if os.name != "nt":
        return False
    try:
        attributes = getattr(path.lstat(), "st_file_attributes", 0)
    except FileNotFoundError:
        return False
    return bool(attributes & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))


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
        if any(type(field.name) is not str or not field.name for field in physical):
            raise ValueError(
                f"Parquet table {table_name!r} must use non-empty column names"
            )
        if first is None:
            first = physical
        elif not physical.equals(first, check_metadata=False):
            raise TypeError(
                f"all Parquet parts for table {table_name!r} must have the same physical schema"
            )
    if first is None:
        raise ValueError(f"table {table_name!r} must contain at least one Parquet file")
    return first
