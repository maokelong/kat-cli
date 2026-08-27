from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import ContextManager, Protocol

import pyarrow as pa


@dataclass(frozen=True, slots=True)
class ParquetSource:
    """One immutable Parquet file or single-table dataset offered by a source."""

    path: Path

    def __post_init__(self) -> None:
        if not isinstance(self.path, Path):
            raise TypeError("ParquetSource path must be a pathlib.Path")


class SourceExecutor(Protocol):
    """Structural source-query port implemented by a PACK Datasource."""

    def execute(
        self,
        sql: str,
        params: object | None,
        *,
        scratch: Path,
    ) -> ContextManager[pa.RecordBatchReader | ParquetSource]: ...

    def close(self) -> None: ...


class Table:
    """An immutable, named relation fully localized by a KAT Provider."""

    __slots__ = (
        "__name",
        "__schema",
        "__operation",
        "__backing_path",
        "__row_count",
    )

    def __new__(cls, *args: object, **kwargs: object) -> Table:
        raise TypeError("Table values can only be created by a KAT Provider")

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Table cannot be subclassed")

    @classmethod
    def _create(
        cls,
        *,
        name: str,
        schema: pa.Schema,
        operation: object,
        backing_path: Path,
        row_count: int,
    ) -> Table:
        table = object.__new__(cls)
        object.__setattr__(table, "_Table__name", name)
        object.__setattr__(table, "_Table__schema", schema)
        object.__setattr__(table, "_Table__operation", operation)
        object.__setattr__(table, "_Table__backing_path", backing_path)
        object.__setattr__(table, "_Table__row_count", row_count)
        return table

    @property
    def name(self) -> str:
        return self.__name

    @property
    def schema(self) -> pa.Schema:
        return self.__schema

    def _runtime_facts(self) -> tuple[object, Path, int]:
        return self.__operation, self.__backing_path, self.__row_count

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Table values are immutable")


class Provider:
    """The KAT-owned, operation-bound facade for one Source executor."""

    __slots__ = ("__query",)

    def __new__(cls, *args: object, **kwargs: object) -> Provider:
        raise TypeError("Provider values can only be created by ctx.provider()")

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Provider cannot be subclassed")

    @classmethod
    def _create(
        cls,
        query: Callable[[str, object | None, str | None], Table],
    ) -> Provider:
        provider = object.__new__(cls)
        object.__setattr__(provider, "_Provider__query", query)
        return provider

    def query(
        self,
        sql: str,
        *,
        params: object | None = None,
        name: str | None = None,
    ) -> Table:
        """Synchronously execute and fully localize one source query."""
        return self.__query(sql, params, name)

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Provider values are immutable")
