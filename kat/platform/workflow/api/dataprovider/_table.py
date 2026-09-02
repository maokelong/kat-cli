from __future__ import annotations

from datetime import datetime, timedelta

import pyarrow as pa

from .._temporal import WallClockTimestamp


_NANOSECONDS_PER_SECOND = 1_000_000_000
_UNIX_EPOCH = datetime(1970, 1, 1)


class Table:
    """An eager, immutable single-table value backed by Arrow."""

    __slots__ = ("__arrow",)

    def __new__(cls, *args: object, **kwargs: object) -> Table:
        raise TypeError("Table values must be created with Table.from_arrow()")

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Table cannot be subclassed")

    @classmethod
    def from_arrow(cls, arrow_table: pa.Table) -> Table:
        _admit_arrow_table(arrow_table)
        table = object.__new__(cls)
        object.__setattr__(table, "_Table__arrow", arrow_table)
        return table

    def __len__(self) -> int:
        return self.__arrow.num_rows

    @property
    def columns(self) -> tuple[str, ...]:
        return tuple(self.__arrow.column_names)

    def __getitem__(self, column_name: str) -> tuple[object | None, ...]:
        if type(column_name) is not str or column_name not in self.__arrow.column_names:
            raise KeyError(column_name)
        return _column_to_python(self.__arrow.column(column_name))

    def to_rows(self) -> list[dict[str, object | None]]:
        names = self.columns
        columns = [_column_to_python(self.__arrow.column(name)) for name in names]
        return [
            {name: columns[column_index][row_index] for column_index, name in enumerate(names)}
            for row_index in range(len(self))
        ]

    def to_arrow(self) -> pa.Table:
        return self.__arrow

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Table attributes are immutable")

    def __delattr__(self, name: str) -> None:
        raise AttributeError("Table attributes are immutable")


def _admit_arrow_table(arrow_table: pa.Table) -> None:
    if not isinstance(arrow_table, pa.Table):
        raise TypeError("standard Table backing must be a pyarrow.Table")
    arrow_table.validate(full=True)
    if arrow_table.num_columns == 0:
        raise ValueError("a standard Table must contain at least one column")

    names = arrow_table.column_names
    if any(type(name) is not str or not name for name in names):
        raise ValueError("standard Table column names must be non-empty strings")
    if len(set(names)) != len(names):
        raise ValueError("standard Table column names must be unique")

    for index, field in enumerate(arrow_table.schema):
        if not _is_admitted_type(field.type):
            raise TypeError(f"column {field.name!r} has unsupported Arrow type {field.type}")
        if not field.nullable and arrow_table.column(index).null_count:
            raise ValueError(
                f"non-nullable column {field.name!r} contains null values"
            )


def _is_string_view(data_type: pa.DataType) -> bool:
    predicate = getattr(pa.types, "is_string_view", None)
    return bool(predicate and predicate(data_type))


def _is_admitted_type(data_type: pa.DataType) -> bool:
    return bool(
        pa.types.is_boolean(data_type)
        or pa.types.is_integer(data_type)
        or pa.types.is_floating(data_type)
        or pa.types.is_string(data_type)
        or pa.types.is_large_string(data_type)
        or _is_string_view(data_type)
        or pa.types.is_binary(data_type)
        or pa.types.is_large_binary(data_type)
        or _is_utc_nanosecond_timestamp(data_type)
        or _is_admitted_decimal(data_type)
    )


def _is_utc_nanosecond_timestamp(data_type: pa.DataType) -> bool:
    return bool(
        pa.types.is_timestamp(data_type)
        and data_type.unit == "ns"
        and data_type.tz == "UTC"
    )


def _is_admitted_decimal(data_type: pa.DataType) -> bool:
    return bool(
        (pa.types.is_decimal128(data_type) or pa.types.is_decimal256(data_type))
        and 0 <= data_type.scale <= data_type.precision
    )


def _column_to_python(column: pa.ChunkedArray) -> tuple[object | None, ...]:
    if _is_utc_nanosecond_timestamp(column.type):
        return tuple(
            None if value is None else WallClockTimestamp(_format_utc_nanoseconds(value))
            for value in column.cast(pa.int64()).to_pylist()
        )
    return tuple(column.to_pylist())


def _format_utc_nanoseconds(value: int) -> str:
    seconds, nanoseconds = divmod(value, _NANOSECONDS_PER_SECOND)
    instant = _UNIX_EPOCH + timedelta(seconds=seconds)
    rendered = (
        f"{instant.year:04d}-{instant.month:02d}-{instant.day:02d}T"
        f"{instant.hour:02d}:{instant.minute:02d}:{instant.second:02d}"
    )
    if nanoseconds:
        rendered += f".{nanoseconds:09d}".rstrip("0")
    return rendered + "Z"
