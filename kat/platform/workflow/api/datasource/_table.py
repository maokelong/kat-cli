from __future__ import annotations

import math
from collections.abc import Mapping
from datetime import datetime, timedelta
from decimal import Decimal

import pyarrow as pa

from .._temporal import WallClockTimestamp, _wall_clock_nanoseconds
from ._schema import (
    _arrow_type,
    _datetime_nanoseconds,
    _freeze_table_schema,
    _logical_type,
    _normalize_python_value,
    _rescale_decimal,
)


_NANOSECONDS_PER_SECOND = 1_000_000_000
_UNIX_EPOCH = datetime(1970, 1, 1)


class Table:
    """An eager, appendable single-table value backed by Arrow chunks."""

    __slots__ = ("__arrow", "__logical_schema", "__pending")

    def __init__(self, schema: Mapping[str, object]) -> None:
        logical_schema = _freeze_table_schema(schema)
        arrow_schema = pa.schema(
            [
                pa.field(
                    name,
                    _arrow_type(annotation),
                    nullable=_logical_type(annotation)[1],
                )
                for name, annotation in logical_schema.items()
            ]
        )
        object.__setattr__(
            self,
            "_Table__arrow",
            pa.Table.from_arrays(
                [pa.chunked_array([], type=field.type) for field in arrow_schema],
                schema=arrow_schema,
            ),
        )
        object.__setattr__(self, "_Table__logical_schema", logical_schema)
        object.__setattr__(self, "_Table__pending", [])

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Table cannot be subclassed")

    @classmethod
    def from_arrow(cls, arrow_table: pa.Table) -> Table:
        _admit_arrow_table(arrow_table)
        table = object.__new__(cls)
        object.__setattr__(table, "_Table__arrow", arrow_table)
        object.__setattr__(table, "_Table__logical_schema", None)
        object.__setattr__(table, "_Table__pending", [])
        return table

    def append(self, **row_values: object | None) -> None:
        names = self.columns
        if len(row_values) != len(names) or set(row_values) != set(names):
            raise ValueError("row values must exactly match the Table columns")

        if self.__logical_schema is not None:
            normalized = tuple(
                _normalize_python_value(
                    *_logical_type(self.__logical_schema[name]),
                    row_values[name],
                    location=f"column {name!r}",
                )
                for name in names
            )
        else:
            normalized = tuple(
                _normalize_physical_value(
                    self.__arrow.schema.field(name),
                    row_values[name],
                    location=f"column {name!r}",
                )
                for name in names
            )
        self.__pending.append(normalized)

    def __len__(self) -> int:
        return self.to_arrow().num_rows

    @property
    def columns(self) -> tuple[str, ...]:
        return tuple(self.__arrow.column_names)

    def __getitem__(self, column_name: str) -> tuple[object | None, ...]:
        if type(column_name) is not str or column_name not in self.__arrow.column_names:
            raise KeyError(column_name)
        return _column_to_python(self.to_arrow().column(column_name))

    def to_rows(self) -> list[dict[str, object | None]]:
        snapshot = self.to_arrow()
        names = self.columns
        columns = [_column_to_python(snapshot.column(name)) for name in names]
        return [
            {name: columns[column_index][row_index] for column_index, name in enumerate(names)}
            for row_index in range(len(self))
        ]

    def to_arrow(self) -> pa.Table:
        if self.__pending:
            arrays = [
                pa.array(
                    [row[column_index] for row in self.__pending],
                    type=field.type,
                )
                for column_index, field in enumerate(self.__arrow.schema)
            ]
            columns = [
                pa.chunked_array(
                    [*self.__arrow.column(index).chunks, arrays[index]],
                    type=field.type,
                )
                for index, field in enumerate(self.__arrow.schema)
            ]
            snapshot = pa.Table.from_arrays(columns, schema=self.__arrow.schema)
            object.__setattr__(self, "_Table__arrow", snapshot)
            self.__pending.clear()
        return self.__arrow

    def __setattr__(self, name: str, value: object) -> None:
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


def _normalize_physical_value(
    field: pa.Field,
    value: object | None,
    *,
    location: str,
) -> object | None:
    if value is None:
        if field.nullable:
            return None
        raise ValueError(f"{location} is null but its column is not nullable")

    data_type = field.type
    if pa.types.is_boolean(data_type):
        _require_exact_type(value, bool, location=location)
        return value
    if pa.types.is_integer(data_type):
        _require_exact_type(value, int, location=location)
        bit_width = data_type.bit_width
        if pa.types.is_signed_integer(data_type):
            minimum = -(2 ** (bit_width - 1))
            maximum = 2 ** (bit_width - 1) - 1
        else:
            minimum = 0
            maximum = 2**bit_width - 1
        if not minimum <= value <= maximum:
            raise ValueError(f"{location} is outside the {data_type} range")
        return value
    if pa.types.is_floating(data_type):
        _require_exact_type(value, float, location=location)
        encoded = pa.scalar(value, type=data_type).as_py()
        if math.isfinite(value) and not math.isfinite(encoded):
            raise ValueError(f"{location} overflows {data_type}")
        return encoded
    if (
        pa.types.is_string(data_type)
        or pa.types.is_large_string(data_type)
        or _is_string_view(data_type)
    ):
        _require_exact_type(value, str, location=location)
        return value
    if pa.types.is_binary(data_type) or pa.types.is_large_binary(data_type):
        _require_exact_type(value, bytes, location=location)
        return value
    if _is_utc_nanosecond_timestamp(data_type):
        if type(value) is datetime:
            return _datetime_nanoseconds(value, location=location)
        if type(value) is WallClockTimestamp:
            return _wall_clock_nanoseconds(value)
        raise TypeError(
            f"{location} must have exact type datetime or WallClockTimestamp, "
            f"got {type(value).__name__}"
        )
    if _is_admitted_decimal(data_type):
        _require_exact_type(value, Decimal, location=location)
        return _rescale_decimal(
            value,
            precision=data_type.precision,
            scale=data_type.scale,
            location=location,
        )
    raise AssertionError(f"admitted Arrow type {data_type} has no append encoding")


def _require_exact_type(value: object, expected: type[object], *, location: str) -> None:
    if type(value) is not expected:
        raise TypeError(
            f"{location} must have exact type {expected.__name__}, "
            f"got {type(value).__name__}"
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
