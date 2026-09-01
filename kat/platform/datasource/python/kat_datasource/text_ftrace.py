from __future__ import annotations

import os
from typing import TypeAlias

__all__ = ("decode", "DecodeError")

_PathLike: TypeAlias = str | os.PathLike[str]


class DecodeError(RuntimeError):
    """文本 Ftrace 无法安全、完整地解码。"""


def decode(
    source: _PathLike,
    destination: _PathLike,
    clock_domain: str,
) -> None:
    from . import _native

    try:
        _native.decode_text_ftrace(
            os.fspath(source),
            os.fspath(destination),
            clock_domain,
        )
    except _native._TextFtraceDecodeError as error:
        raise DecodeError(str(error)) from None
