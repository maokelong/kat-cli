from __future__ import annotations

import re
from datetime import datetime, timedelta
from decimal import Decimal, InvalidOperation


_DURATION = re.compile(r"([0-9]+(?:\.[0-9]{1,9})?)(ns|us|ms|s|min|h)\Z")
_DURATION_FACTORS = {
    "ns": 1,
    "us": 1_000,
    "ms": 1_000_000,
    "s": 1_000_000_000,
    "min": 60_000_000_000,
    "h": 3_600_000_000_000,
}
_MAX_DURATION_NS = 2**63 - 1
_MIN_TIMESTAMP_NS = -(2**63)
_MAX_TIMESTAMP_NS = 2**63 - 1
_WALL_CLOCK = re.compile(
    r"(?P<date>[0-9]{4}-[0-9]{2}-[0-9]{2})T"
    r"(?P<time>[0-9]{2}:[0-9]{2}:[0-9]{2})"
    r"(?P<fraction>\.[0-9]{1,9})?"
    r"(?P<offset>Z|[+-][0-9]{2}:[0-9]{2})\Z"
)


class Duration(str):
    """A non-negative decimal elapsed time with one supported unit suffix."""

    def __new__(cls, literal: str) -> Duration:
        if type(literal) is not str:
            raise TypeError("Duration requires a string literal")
        _duration_nanoseconds(literal)
        return str.__new__(cls, literal)


class WallClockTimestamp(str):
    """An absolute UTC instant parsed from offset-bearing RFC 3339 text.

    "Wall-clock" distinguishes globally aligned calendar timestamps from
    trace-local clock-domain readings. It is not a local civil-time value: the
    required offset is consumed when the value is normalized to ``Z``.
    """

    def __new__(cls, literal: str) -> WallClockTimestamp:
        if type(literal) is not str:
            raise TypeError("WallClockTimestamp requires a string literal")
        match = _WALL_CLOCK.fullmatch(literal)
        if match is None:
            raise ValueError(f"invalid WallClockTimestamp literal: {literal!r}")
        try:
            local = datetime.strptime(
                f"{match.group('date')}T{match.group('time')}", "%Y-%m-%dT%H:%M:%S"
            )
            offset = match.group("offset")
            if offset == "-00:00":
                raise ValueError("unknown UTC offset is not allowed")
            if offset != "Z":
                hours, minutes = map(int, offset[1:].split(":"))
                if hours > 23 or minutes > 59:
                    raise ValueError("invalid UTC offset")
                delta = timedelta(hours=hours, minutes=minutes)
                local = local - delta if offset[0] == "+" else local + delta
        except (OverflowError, ValueError) as error:
            raise ValueError(f"invalid WallClockTimestamp literal: {literal!r}") from error
        fraction = (match.group("fraction") or "")[1:].ljust(9, "0").rstrip("0")
        normalized = local.strftime("%Y-%m-%dT%H:%M:%S")
        if fraction:
            normalized += f".{fraction}"
        nanoseconds = _wall_clock_nanoseconds(normalized + "Z")
        if not _MIN_TIMESTAMP_NS <= nanoseconds <= _MAX_TIMESTAMP_NS:
            raise ValueError(
                "WallClockTimestamp is outside Arrow timestamp(ns) range: "
                f"{literal!r}"
            )
        return str.__new__(cls, normalized + "Z")


def _duration_nanoseconds(value: str) -> int:
    match = _DURATION.fullmatch(value)
    if match is None:
        raise ValueError(f"invalid Duration literal: {value!r}")
    try:
        nanoseconds = Decimal(match.group(1)) * _DURATION_FACTORS[match.group(2)]
    except InvalidOperation as error:
        raise ValueError(f"invalid Duration literal: {value!r}") from error
    if nanoseconds != nanoseconds.to_integral_value() or not 0 <= nanoseconds <= _MAX_DURATION_NS:
        raise ValueError(
            f"Duration is not an exact non-negative int64 nanosecond value: {value!r}"
        )
    return int(nanoseconds)


def _wall_clock_nanoseconds(value: str) -> int:
    base, _, fraction = value[:-1].partition(".")
    instant = datetime.fromisoformat(base)
    delta = instant - datetime(1970, 1, 1)
    seconds = delta.days * 86_400 + delta.seconds
    return seconds * 1_000_000_000 + int(fraction.ljust(9, "0") or "0")
