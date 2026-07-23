from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
from typing import Iterator, Literal, NotRequired, TypedDict

from .pack import InspectPackRuntimeResult, PackInspectionError, inspect_pack


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


type InspectPackRuntimeResponse = RuntimeSuccess[InspectPackRuntimeResult] | RuntimeFailure


@dataclass(frozen=True)
class InspectPackRequest:
    pack_name: str
    pack_path: Path


class RuntimeRequestError(Exception):
    """The control file does not contain a valid Runtime Request."""


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--request", required=True)
    parser.add_argument("--response", required=True)
    arguments = parser.parse_args()
    response_path = Path(arguments.response)
    try:
        request = _read_request(Path(arguments.request))
    except RuntimeRequestError as error:
        response: InspectPackRuntimeResponse = RuntimeFailure(
            error=_diagnostic(
                error,
                None,
                message="Runtime Request is invalid",
                help="Use a compatible KAT CLI and Runtime deployment",
            )
        )
    else:
        pack_path = request.pack_path
        try:
            result = inspect_pack(request.pack_name, request.pack_path)
        except PackInspectionError as error:
            response = RuntimeFailure(
                error=_diagnostic(
                    error,
                    pack_path,
                    message="PACK inspection failed",
                    help="Correct the PACK production Interface and retry inspection",
                )
            )
        else:
            response = RuntimeSuccess(result=result)
    _write_response(response_path, response)
    return 0


def _read_request(path: Path) -> InspectPackRequest:
    with path.open("r", encoding="utf-8") as file:
        try:
            request = json.load(file)
        except (json.JSONDecodeError, UnicodeError) as error:
            raise RuntimeRequestError("Runtime Request must be UTF-8 JSON") from error
    if type(request) is not dict:
        raise RuntimeRequestError("Runtime Request must be a JSON object")
    expected = {"operation", "pack_name", "pack_path"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"inspect_pack Runtime Request fields must be exactly {sorted(expected)}"
        )
    if request["operation"] != "inspect_pack":
        raise RuntimeRequestError("unsupported Runtime Request operation")
    if type(request["pack_name"]) is not str or type(request["pack_path"]) is not str:
        raise RuntimeRequestError("inspect_pack Runtime Request fields must be strings")
    if not request["pack_name"]:
        raise RuntimeRequestError("inspect_pack Runtime Request PACK name must not be empty")
    supplied = Path(request["pack_path"])
    if not supplied.is_absolute():
        raise RuntimeRequestError("inspect_pack Runtime Request PACK path must be absolute")
    try:
        pack_path = supplied.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        raise RuntimeRequestError(
            "inspect_pack Runtime Request PACK path must identify an existing directory"
        ) from error
    if pack_path != supplied or not pack_path.is_dir():
        raise RuntimeRequestError(
            "inspect_pack Runtime Request PACK path must identify its canonical directory"
        )
    return InspectPackRequest(pack_name=request["pack_name"], pack_path=pack_path)


def _diagnostic(
    error: Exception,
    pack_path: Path | None,
    *,
    message: str,
    help: str,
) -> RuntimeDiagnostic:
    causes: list[str] = []
    for current in _exception_chain(error):
        try:
            rendered = str(current).strip()
        except BaseException:
            continue
        if rendered:
            causes.append(rendered)
    diagnostic: RuntimeDiagnostic = {"message": message, "help": help}
    if causes:
        diagnostic["causes"] = causes
    try:
        location = _syntax_error_location(error, pack_path)
    except BaseException:
        location = None
    if location is not None:
        diagnostic["location"] = location
    return diagnostic


def _syntax_error_location(
    error: Exception, pack_path: Path | None
) -> DiagnosticLocation | None:
    syntax_error = next(
        (current for current in _exception_chain(error) if isinstance(current, SyntaxError)),
        None,
    )
    if syntax_error is None or pack_path is None or syntax_error.filename is None:
        return None
    error = syntax_error
    positions = (error.lineno, error.offset, error.end_lineno, error.end_offset)
    if any(type(value) is not int or value <= 0 for value in positions):
        return None
    start_line, start_column, end_line, end_column = positions
    if (end_line, end_column) < (start_line, start_column):
        return None
    try:
        root = pack_path.resolve(strict=True)
        relative = Path(error.filename).resolve(strict=True).relative_to(root)
    except (OSError, ValueError):
        return None
    if relative == Path():
        return None
    return {
        "source": relative.as_posix(),
        "start": {"line": start_line, "column": start_column},
        "end": {"line": end_line, "column": end_column},
    }


def _exception_chain(error: BaseException) -> Iterator[BaseException]:
    seen: set[int] = set()
    current: BaseException | None = error
    while current is not None and id(current) not in seen:
        seen.add(id(current))
        yield current
        cause = BaseException.__cause__.__get__(current, BaseException)
        if cause is not None:
            current = cause
        elif BaseException.__suppress_context__.__get__(current, BaseException):
            current = None
        else:
            current = BaseException.__context__.__get__(current, BaseException)


def _write_response(path: Path, response: InspectPackRuntimeResponse) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(asdict(response), file, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


if __name__ == "__main__":
    raise SystemExit(main())
