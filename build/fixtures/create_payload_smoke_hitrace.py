#!/usr/bin/env python3
"""Create the smallest deterministic Hitrace input used by Payload smoke."""

from __future__ import annotations

import struct
import sys
from pathlib import Path


PROFILER_HEADER_SIZE = 1024
PROFILER_HEADER_MAGIC = 0x464F5250534F484F


def create_fixture(path: Path) -> None:
    content = bytearray(PROFILER_HEADER_SIZE)
    struct.pack_into("<Q", content, 0, PROFILER_HEADER_MAGIC)
    struct.pack_into("<Q", content, 8, PROFILER_HEADER_SIZE)
    struct.pack_into("<I", content, 56, 0)
    struct.pack_into("<Q", content, 60, 123456)
    path.write_bytes(content)


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: create_payload_smoke_hitrace.py OUTPUT", file=sys.stderr)
        return 2
    create_fixture(Path(argv[0]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
