from __future__ import annotations

import os
from dataclasses import dataclass
from typing import TypeAlias

__all__ = (
    "decode",
    "DecodeReport",
    "DecodeError",
    "HEADER_RELATION",
    "OCCURRENCE_RELATION",
    "EVENT_RELATION",
    "UNSUPPORTED_EVENT_RELATION",
)

HEADER_RELATION = "text_ftrace_header"
OCCURRENCE_RELATION = "text_ftrace_event_occurrence"
EVENT_RELATION = "text_ftrace_event"
UNSUPPORTED_EVENT_RELATION = "text_ftrace_unsupported_event"

_PathLike: TypeAlias = str | os.PathLike[str]


class DecodeError(RuntimeError):
    """文本 Ftrace 无法安全、完整地解码。"""


@dataclass(frozen=True, slots=True)
class DecodeReport:
    unsupported_event_names: tuple[str, ...]


def decode(
    source: _PathLike,
    destination: _PathLike,
    clock_domain: str,
) -> DecodeReport:
    from . import _native

    try:
        unsupported_event_names = _native.decode_text_ftrace(
            os.fspath(source),
            os.fspath(destination),
            clock_domain,
        )
    except _native._TextFtraceDecodeError as error:
        raise DecodeError(str(error)) from None
    return DecodeReport(unsupported_event_names=tuple(unsupported_event_names))
