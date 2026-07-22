from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
from typing import Literal, NotRequired, TypedDict

from .pack import InspectPackResult, inspect_pack


class DiagnosticPosition(TypedDict):
    line: int
    column: int


class DiagnosticLocation(TypedDict):
    source: str
    start: DiagnosticPosition
    end: DiagnosticPosition


class RuntimeDiagnostic(TypedDict):
    message: str
    help: str
    causes: NotRequired[list[str]]
    location: NotRequired[DiagnosticLocation]


@dataclass(frozen=True)
class RuntimeSuccess[R]:
    status: Literal["success"] = field(init=False, default="success")
    result: R


@dataclass(frozen=True)
class RuntimeFailure:
    status: Literal["failure"] = field(init=False, default="failure")
    error: RuntimeDiagnostic


type InspectPackRuntimeResponse = RuntimeSuccess[InspectPackResult] | RuntimeFailure


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--request", required=True)
    parser.add_argument("--response", required=True)
    arguments = parser.parse_args()
    response_path = Path(arguments.response)
    pack_path: Path | None = None
    try:
        request = _read_request(Path(arguments.request))
        pack_path = Path(request["pack_path"])
        result = inspect_pack(request["pack_name"], request["pack_path"])
        response: InspectPackRuntimeResponse = RuntimeSuccess(result=result)
    except Exception as error:
        response = RuntimeFailure(error=_diagnostic(error, pack_path))
    _write_response(response_path, response)
    return 0


def _read_request(path: Path) -> dict[str, str]:
    with path.open("r", encoding="utf-8") as file:
        request = json.load(file)
    if type(request) is not dict:
        raise ValueError("Runtime Request must be a JSON object")
    expected = {"operation", "pack_name", "pack_path"}
    if set(request) != expected:
        raise ValueError(f"inspect_pack Runtime Request fields must be exactly {sorted(expected)}")
    if request["operation"] != "inspect_pack":
        raise ValueError("unsupported Runtime Request operation")
    if type(request["pack_name"]) is not str or type(request["pack_path"]) is not str:
        raise TypeError("inspect_pack Runtime Request fields must be strings")
    return request


def _diagnostic(error: Exception, pack_path: Path | None) -> RuntimeDiagnostic:
    causes: list[str] = []
    current: BaseException | None = error
    while current is not None:
        rendered = str(current).strip()
        if rendered:
            causes.append(rendered)
        current = current.__cause__ or current.__context__
    diagnostic: RuntimeDiagnostic = {
        "message": "PACK inspection failed",
        "help": "Correct the PACK production Interface and retry inspection",
    }
    if causes:
        diagnostic["causes"] = causes
    location = _syntax_error_location(error, pack_path)
    if location is not None:
        diagnostic["location"] = location
    return diagnostic


def _syntax_error_location(
    error: Exception, pack_path: Path | None
) -> DiagnosticLocation | None:
    if not isinstance(error, SyntaxError) or pack_path is None or error.filename is None:
        return None
    positions = (error.lineno, error.offset, error.end_lineno, error.end_offset)
    if any(type(value) is not int or value <= 0 for value in positions):
        return None
    start_line, start_column, end_line, end_column = positions
    if (end_line, end_column) < (start_line, start_column):
        return None
    try:
        root = pack_path.resolve(strict=True)
        source = Path(error.filename).resolve(strict=True).relative_to(root).as_posix()
    except (OSError, ValueError):
        return None
    return {
        "source": source,
        "start": {"line": start_line, "column": start_column},
        "end": {"line": end_line, "column": end_column},
    }


def _write_response(path: Path, response: InspectPackRuntimeResponse) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(asdict(response), file, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


if __name__ == "__main__":
    raise SystemExit(main())
