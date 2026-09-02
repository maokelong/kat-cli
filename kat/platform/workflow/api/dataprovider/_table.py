from __future__ import annotations

from collections.abc import Iterable, Mapping
from datetime import datetime, timedelta
from decimal import Decimal
import math

import pyarrow as pa

from .._temporal import WallClockTimestamp, _wall_clock_nanoseconds
from ._schema import _datetime_nanoseconds, _require_utf8_text, _rescale_decimal


_NANOSECONDS_PER_SECOND = 1_000_000_000
_UNIX_EPOCH = datetime(1970, 1, 1)


class Table:
    """An eager, immutable single-table value backed by Arrow."""

    __slots__ = ("__arrow",)

    def __new__(cls, *args: object, **kwargs: object) -> Table:
        raise TypeError(
            "Table values must be created with Table.from_arrow() or Table.from_rows()"
        )

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Table cannot be subclassed")

    @classmethod
    def from_arrow(cls, arrow_table: pa.Table) -> Table:
        _admit_arrow_table(arrow_table)
        table = object.__new__(cls)
        object.__setattr__(table, "_Table__arrow", arrow_table)
        return table

    @classmethod
    def from_rows(
        cls,
        rows: Iterable[Mapping[str, object | None]],
        *,
        schema: pa.Schema,
    ) -> Table:
        _admit_arrow_schema(schema)
        names = tuple(schema.names)
        columns: list[list[object | None]] = [[] for _ in schema]
        for row_index, row in enumerate(rows):
            if not isinstance(row, Mapping):
                raise TypeError(f"Table row {row_index} must be a mapping")
            if len(row) != len(names) or set(row) != set(names):
                raise ValueError(
                    f"Table row {row_index} fields must exactly match the Table columns"
                )
            normalized = tuple(
                _normalize_physical_value(
                    field,
                    row[field.name],
                    location=f"Table row {row_index}, column {field.name!r}",
                )
                for field in schema
            )
            for column, value in zip(columns, normalized, strict=True):
                column.append(value)
        arrays = [
            pa.array(column, type=field.type)
            for column, field in zip(columns, schema, strict=True)
        ]
        return cls.from_arrow(pa.Table.from_arrays(arrays, schema=schema))

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
    _admit_arrow_schema(arrow_table.schema)

    for index, field in enumerate(arrow_table.schema):
        if not field.nullable and arrow_table.column(index).null_count:
            raise ValueError(
                f"non-nullable column {field.name!r} contains null values"
            )


def _admit_arrow_schema(arrow_schema: pa.Schema) -> None:
    if not isinstance(arrow_schema, pa.Schema):
        raise TypeError("standard Table row schema must be a pyarrow.Schema")
    if len(arrow_schema) == 0:
        raise ValueError("a standard Table must contain at least one column")

    names = arrow_schema.names
    if any(type(name) is not str or not name for name in names):
        raise ValueError("standard Table column names must be non-empty strings")
    if len(set(names)) != len(names):
        raise ValueError("standard Table column names must be unique")

    for field in arrow_schema:
        if not _is_admitted_type(field.type):
            raise TypeError(f"column {field.name!r} has unsupported Arrow type {field.type}")


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
        return _require_utf8_text(value, location=location)
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
    raise AssertionError(f"admitted Arrow type {data_type} has no row encoding")


def _require_exact_type(
    value: object,
    expected: type[object],
    *,
    location: str,
) -> None:
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
