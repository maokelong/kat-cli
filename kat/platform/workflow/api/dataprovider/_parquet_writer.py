from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq


@dataclass(frozen=True, slots=True)
class _ParquetRelationMetadata:
    schema: pa.Schema
    row_count: int


class _ParquetRelationWriter:
    """Write one completed Parquet relation without owning publication."""

    __slots__ = ("__path", "__row_count", "__schema", "__writer")

    def __init__(
        self,
        path: Path,
        schema: pa.Schema,
        *,
        compression: str = "snappy",
    ) -> None:
        self.__path = path
        self.__row_count = 0
        self.__schema = schema
        self.__writer = pq.ParquetWriter(path, schema, compression=compression)

    def write_rows(
        self,
        rows: tuple[tuple[object | None, ...], ...],
    ) -> None:
        arrays = [
            pa.array([row[index] for row in rows], type=field.type)
            for index, field in enumerate(self.__schema)
        ]
        table = pa.Table.from_arrays(arrays, schema=self.__schema)
        self.write_table(table, row_group_size=len(rows))

    def write_table(
        self,
        table: pa.Table,
        *,
        row_group_size: int | None = None,
    ) -> None:
        if not table.schema.equals(self.__schema, check_metadata=False):
            raise TypeError("Arrow table schema must match the Parquet relation schema")
        self.__writer.write_table(table, row_group_size=row_group_size)
        self.__row_count += table.num_rows

    def close(self) -> _ParquetRelationMetadata:
        self.__writer.close()
        metadata = pq.read_metadata(self.__path)
        physical_schema = metadata.schema.to_arrow_schema()
        if not physical_schema.equals(self.__schema, check_metadata=False):
            raise RuntimeError(
                "Parquet footer schema does not match the relation schema"
            )
        if metadata.num_rows != self.__row_count:
            raise RuntimeError("Parquet footer row count does not match accepted rows")
        return _ParquetRelationMetadata(physical_schema, metadata.num_rows)
