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
        match = _DURATION.fullmatch(literal)
        if match is None:
            raise ValueError(f"invalid Duration literal: {literal!r}")
        try:
            nanoseconds = Decimal(match.group(1)) * _DURATION_FACTORS[match.group(2)]
        except InvalidOperation as error:
            raise ValueError(f"invalid Duration literal: {literal!r}") from error
        if nanoseconds != nanoseconds.to_integral_value() or not 0 <= nanoseconds <= _MAX_DURATION_NS:
            raise ValueError(f"Duration is not an exact non-negative int64 nanosecond value: {literal!r}")
        return str.__new__(cls, literal)


class WallClockTimestamp(str):
    """An RFC 3339 instant with a known offset, normalized to UTC nanoseconds."""

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
        return str.__new__(cls, normalized + "Z")
