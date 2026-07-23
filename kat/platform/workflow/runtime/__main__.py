from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
from typing import Literal

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .pack import InspectPackRuntimeResult, PackInspectionError, inspect_pack


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
            error=diagnostic_from_exception(
                error,
                None,
                message="Runtime Request is invalid",
                help="Use a compatible KAT CLI and Runtime deployment",
            )
        )
    else:
        try:
            result = inspect_pack(request.pack_name, request.pack_path)
        except PackInspectionError as error:
            response = RuntimeFailure(error=error.diagnostic)
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


def _write_response(path: Path, response: InspectPackRuntimeResponse) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(asdict(response), file, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


if __name__ == "__main__":
    raise SystemExit(main())
