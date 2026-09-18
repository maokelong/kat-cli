from __future__ import annotations

import re


_IDENTIFIER = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")
_WINDOWS_DEVICES = {"con", "prn", "aux", "nul"} | {
    f"{prefix}{number}" for prefix in ("com", "lpt") for number in range(1, 10)
}


def valid_output_name(name: str) -> bool:
    return _IDENTIFIER.fullmatch(name) is not None and name not in _WINDOWS_DEVICES


def valid_table_name(name: str) -> bool:
    return valid_output_name(name)
