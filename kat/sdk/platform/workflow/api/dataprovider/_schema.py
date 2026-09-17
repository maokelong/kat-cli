from __future__ import annotations

import types
from collections.abc import Mapping
from datetime import datetime
from decimal import Decimal
from types import MappingProxyType
from typing import get_args, get_origin

import pyarrow as pa

from .._identifiers import valid_table_name
from .._temporal import _MAX_TIMESTAMP_NS, _MIN_TIMESTAMP_NS

_LOGICAL_TYPES = (bool, int, float, str, bytes, datetime, Decimal)
_INT64_MIN = -(2**63)
_INT64_MAX = 2**63 - 1
_UNIX_EPOCH = datetime(1970, 1, 1)
_DECIMAL_SCALE = 18
_DECIMAL_PRECISION = 38


class Schema:
    """An immutable, ordered declaration of logical Datasource tables."""

    __slots__ = ("__declarations", "__tables")

    def __init_subclass__(cls, **kwargs: object) -> None:
        raise TypeError("Schema cannot be subclassed")

    def __init__(self, tables: Mapping[str, Mapping[str, object]]) -> None:
        if not isinstance(tables, Mapping):
            raise TypeError("Datasource Schema requires a mapping of tables")

        declarations: dict[str, Mapping[str, object]] = {}
        for table_name, columns in list(tables.items()):
            if type(table_name) is not str or not valid_table_name(table_name):
                raise ValueError(
                    f"invalid Datasource table name {table_name!r}; expected a file-safe "
                    "lowercase name"
                )
            declarations[table_name] = _freeze_table_schema(columns)
        if not declarations:
            raise ValueError("Datasource Schema must declare at least one table")

        object.__setattr__(
            self,
            "_Schema__declarations",
            MappingProxyType(declarations),
        )
        object.__setattr__(self, "_Schema__tables", tuple(declarations))

    @property
    def tables(self) -> tuple[str, ...]:
        return self.__tables

    def __getitem__(self, table_name: str) -> Mapping[str, object]:
        return self.__declarations[table_name]

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError("Schema values are immutable")


def _freeze_table_schema(schema: Mapping[str, object]) -> Mapping[str, object]:
    if not isinstance(schema, Mapping):
        raise TypeError("a table schema must be a mapping of columns")

    columns: dict[str, object] = {}
    for column_name, annotation in list(schema.items()):
        if type(column_name) is not str or not column_name:
            raise ValueError("Datasource column names must be non-empty strings")
        _require_utf8_text(column_name, location="Datasource column name")
        _logical_type(annotation)
        columns[column_name] = annotation
    if not columns:
        raise ValueError("a table schema must declare at least one column")
    return MappingProxyType(columns)


def _logical_type(annotation: object) -> tuple[type[object], bool]:
    for logical_type in _LOGICAL_TYPES:
        if annotation is logical_type:
            return logical_type, False  # type: ignore[return-value]

    if get_origin(annotation) is types.UnionType:
        arguments = get_args(annotation)
        non_null = tuple(argument for argument in arguments if argument is not type(None))
        if len(arguments) == 2 and len(non_null) == 1:
            for logical_type in _LOGICAL_TYPES:
                if non_null[0] is logical_type:
                    return logical_type, True  # type: ignore[return-value]

    raise TypeError(
        "Datasource columns must use bool, int, float, str, bytes, datetime, "
        "Decimal, or one of those types unioned with None"
    )


def _arrow_type(annotation: object) -> pa.DataType:
    logical_type, _ = _logical_type(annotation)
    if logical_type is bool:
        return pa.bool_()
    if logical_type is int:
        return pa.int64()
    if logical_type is float:
        return pa.float64()
    if logical_type is str:
        return pa.string()
    if logical_type is bytes:
        return pa.binary()
    if logical_type is datetime:
        return pa.timestamp("ns", tz="UTC")
    if logical_type is Decimal:
        return pa.decimal128(_DECIMAL_PRECISION, _DECIMAL_SCALE)
    raise AssertionError("validated logical type is not mapped to Arrow")


def _logical_arrow_schema(logical_schema: Mapping[str, object]) -> pa.Schema:
    return pa.schema(
        [
            pa.field(
                name,
                _arrow_type(annotation),
                nullable=_logical_type(annotation)[1],
            )
            for name, annotation in logical_schema.items()
        ]
    )


def _normalize_logical_row(
    logical_schema: Mapping[str, object],
    row_values: Mapping[str, object | None],
) -> tuple[object | None, ...]:
    names = tuple(logical_schema)
    if len(row_values) != len(names) or set(row_values) != set(names):
        raise ValueError("row values must exactly match the Datasource table columns")
    return tuple(
        _normalize_python_value(
            *_logical_type(logical_schema[name]),
            row_values[name],
            location=f"column {name!r}",
        )
        for name in names
    )


def _normalize_python_value(
    logical_type: type[object],
    nullable: bool,
    value: object | None,
    *,
    location: str,
) -> object | None:
    if value is None:
        if nullable:
            return None
        raise ValueError(f"{location} is null but its column is not nullable")

    if type(value) is not logical_type:
        raise TypeError(
            f"{location} must have exact type {logical_type.__name__}, "
            f"got {type(value).__name__}"
        )
    if logical_type is int:
        if not _INT64_MIN <= value <= _INT64_MAX:  # type: ignore[operator]
            raise ValueError(f"{location} is outside the signed int64 range")
        return value
    if logical_type is str:
        return _require_utf8_text(value, location=location)  # type: ignore[arg-type]
    if logical_type is datetime:
        return _datetime_nanoseconds(value, location=location)  # type: ignore[arg-type]
    if logical_type is Decimal:
        return _canonical_decimal(value, location=location)  # type: ignore[arg-type]
    return value


def _require_utf8_text(value: str, *, location: str) -> str:
    try:
        value.encode("utf-8")
    except UnicodeEncodeError:
        raise ValueError(f"{location} must be valid UTF-8 text") from None
    return value


def _datetime_nanoseconds(value: datetime, *, location: str) -> int:
    try:
        offset = value.utcoffset()
    except Exception as error:
        raise ValueError(f"{location} must have a valid UTC offset") from error
    if offset is None:
        raise ValueError(f"{location} must be an aware datetime with a UTC offset")

    try:
        utc_value = value.replace(tzinfo=None) - offset
    except (OverflowError, ValueError) as error:
        raise ValueError(f"{location} cannot be normalized to UTC") from error
    delta = utc_value - _UNIX_EPOCH
    nanoseconds = (
        (delta.days * 86_400 + delta.seconds) * 1_000_000_000
        + delta.microseconds * 1_000
    )
    if not _MIN_TIMESTAMP_NS <= nanoseconds <= _MAX_TIMESTAMP_NS:
        raise ValueError(f"{location} is outside the signed int64 timestamp(ns) range")
    return nanoseconds


def _canonical_decimal(value: Decimal, *, location: str) -> Decimal:
    return _rescale_decimal(
        value,
        precision=_DECIMAL_PRECISION,
        scale=_DECIMAL_SCALE,
        location=location,
    )


def _rescale_decimal(
    value: Decimal,
    *,
    precision: int,
    scale: int,
    location: str,
) -> Decimal:
    if not value.is_finite():
        raise ValueError(f"{location} must be a finite Decimal")

    decimal_tuple = value.as_tuple()
    digits = "".join(str(digit) for digit in decimal_tuple.digits)
    if not digits or not any(digit != "0" for digit in digits):
        return Decimal((decimal_tuple.sign, (0,), -scale))

    shift = decimal_tuple.exponent + scale
    if shift >= 0:
        if len(digits) + shift > precision:
            raise ValueError(
                f"{location} exceeds Decimal precision {precision} and scale {scale}"
            )
        scaled_digits = digits + "0" * shift
    else:
        removed = -shift
        if removed >= len(digits) or any(digit != "0" for digit in digits[-removed:]):
            raise ValueError(
                f"{location} cannot be rescaled to {scale} fractional digits "
                "without rounding"
            )
        scaled_digits = digits[:-removed]

    if len(scaled_digits) > precision:
        raise ValueError(
            f"{location} exceeds Decimal precision {precision} and scale {scale}"
        )
    return Decimal(
        (
            decimal_tuple.sign,
            tuple(int(digit) for digit in scaled_digits),
            -scale,
        )
    )
