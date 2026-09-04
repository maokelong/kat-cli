from __future__ import annotations

import os
from dataclasses import dataclass
from typing import TypeAlias

__all__ = (
    "decode",
    "DecodeReport",
    "DecodeError",
    "MATERIALIZATION_VERSION_METADATA_KEY",
    "MATERIALIZATION_VERSION",
)

MATERIALIZATION_VERSION_METADATA_KEY = b"kat.materialization.version"
MATERIALIZATION_VERSION = "hitrace-v1"

_PathLike: TypeAlias = str | os.PathLike[str]


@dataclass(frozen=True, slots=True)
class DecodeReport:
    unsupported_plugins: tuple[str, ...]
    unsupported_section_types: tuple[int, ...]


class DecodeError(RuntimeError):
    """Hitrace 无法安全、完整地解码。"""


def decode(source: _PathLike, destination: _PathLike) -> DecodeReport:
    from . import _native

    try:
        unsupported_plugins, unsupported_section_types = _native.decode(
            os.fspath(source),
            os.fspath(destination),
        )
    except _native._DecodeError as error:
        raise DecodeError(str(error)) from None
    return DecodeReport(
        unsupported_plugins=tuple(unsupported_plugins),
        unsupported_section_types=tuple(unsupported_section_types),
    )
