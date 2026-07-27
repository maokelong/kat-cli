from __future__ import annotations

from dataclasses import dataclass
import json
import logging
from pathlib import Path
import uuid

from kat._identifiers import valid_table_name


_LOGGER = logging.getLogger(__name__)


class RuntimeRequestError(Exception):
    """The control file does not contain a valid Runtime Request."""


@dataclass(frozen=True)
class InspectPackRequest:
    pack_name: str
    pack_path: Path


@dataclass(frozen=True)
class ResolvedDatasetRef:
    path: Path
    tables: dict[str, Path]


@dataclass(frozen=True)
class RunCandidateRef:
    identifier: str
    path: Path


@dataclass(frozen=True)
class RunWorkflowRequest:
    pack_name: str
    pack_path: Path
    workflow_name: str
    dataset: ResolvedDatasetRef | None
    arguments: list[str]
    candidate: RunCandidateRef


type RuntimeRequest = InspectPackRequest | RunWorkflowRequest


def read_request(path: Path) -> RuntimeRequest:
    with path.open("r", encoding="utf-8") as file:
        try:
            request = json.load(file)
        except (json.JSONDecodeError, UnicodeError) as error:
            raise RuntimeRequestError("Runtime Request must be UTF-8 JSON") from error
    if type(request) is not dict:
        raise RuntimeRequestError("Runtime Request must be a JSON object")
    operation = request.get("operation")
    if operation == "inspect_pack":
        return _read_inspect_pack_request(request)
    if operation == "run_workflow":
        return _read_run_workflow_request(request)
    raise RuntimeRequestError("unsupported Runtime Request operation")


def _read_inspect_pack_request(request: dict[str, object]) -> InspectPackRequest:
    expected = {"operation", "pack_name", "pack_path"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"inspect_pack Runtime Request fields must be exactly {sorted(expected)}"
        )
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    if type(pack_name) is not str or type(pack_path) is not str:
        raise RuntimeRequestError("inspect_pack Runtime Request fields must be strings")
    if not pack_name:
        raise RuntimeRequestError("inspect_pack Runtime Request PACK name must not be empty")
    return InspectPackRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(pack_path, "inspect_pack PACK"),
    )


def _read_run_workflow_request(request: dict[str, object]) -> RunWorkflowRequest:
    required = {
        "operation",
        "pack_name",
        "pack_path",
        "workflow_name",
        "arguments",
        "candidate_id",
        "candidate_path",
    }
    if set(request) not in (required, required | {"dataset"}):
        raise RuntimeRequestError("run_workflow Runtime Request has an invalid field set")
    strings = required - {"operation", "arguments"}
    if any(type(request[name]) is not str or not request[name] for name in strings):
        raise RuntimeRequestError(
            "run_workflow identity and path fields must be non-empty strings"
        )
    arguments = request["arguments"]
    if type(arguments) is not list or any(type(value) is not str for value in arguments):
        raise RuntimeRequestError("run_workflow arguments must be an array of strings")
    dataset = (
        _read_resolved_dataset(request["dataset"])
        if "dataset" in request
        else None
    )
    return RunWorkflowRequest(
        pack_name=request["pack_name"],
        pack_path=_canonical_directory(request["pack_path"], "run_workflow PACK"),
        workflow_name=request["workflow_name"],
        dataset=dataset,
        arguments=arguments,
        candidate=_read_run_candidate(
            request["candidate_id"],
            request["candidate_path"],
        ),
    )


def _read_resolved_dataset(value: object) -> ResolvedDatasetRef:
    if type(value) is not dict or set(value) != {"path", "tables"}:
        raise RuntimeRequestError(
            "run_workflow Dataset must contain exactly path and tables"
        )
    path = value["path"]
    tables = value["tables"]
    if type(path) is not str or type(tables) is not dict:
        raise RuntimeRequestError(
            "run_workflow Dataset path and tables have invalid types"
        )
    root = _canonical_directory(path, "run_workflow Dataset")
    resolved_tables: dict[str, Path] = {}
    for name, table_path in tables.items():
        if (
            type(name) is not str
            or not valid_table_name(name)
            or type(table_path) is not str
        ):
            raise RuntimeRequestError(
                "run_workflow Dataset table references are invalid"
            )
        resolved = _canonical_file(table_path, "run_workflow Dataset table")
        if not resolved.is_relative_to(root):
            raise RuntimeRequestError(
                "run_workflow Dataset table references must remain inside the Dataset"
            )
        resolved_tables[name] = resolved
    return ResolvedDatasetRef(path=root, tables=resolved_tables)


def _read_run_candidate(candidate_id: str, candidate_path: str) -> RunCandidateRef:
    try:
        identity = uuid.UUID(candidate_id)
    except ValueError:
        raise RuntimeRequestError(
            "run_workflow candidate identity is invalid"
        ) from None
    if identity.version != 7 or str(identity) != candidate_id:
        raise RuntimeRequestError("run_workflow candidate identity is invalid")
    path = _canonical_directory(candidate_path, "run_workflow candidate")
    if path.name != candidate_id or (path / "manifest.json").exists():
        raise RuntimeRequestError(
            "run_workflow candidate identity and directory do not match"
        )
    return RunCandidateRef(identifier=candidate_id, path=path)


def _canonical_directory(value: str, label: str) -> Path:
    return _canonical_path(value, label, directory=True)


def _canonical_file(value: str, label: str) -> Path:
    return _canonical_path(value, label, directory=False)


def _canonical_path(value: str, label: str, *, directory: bool) -> Path:
    supplied = Path(value)
    if not supplied.is_absolute():
        raise RuntimeRequestError(f"{label} path must be absolute")
    try:
        resolved = supplied.resolve(strict=True)
    except (OSError, RuntimeError):
        _LOGGER.exception("failed to resolve private Runtime Request path for %s", label)
        raise RuntimeRequestError(f"{label} path must exist") from None
    correct_kind = resolved.is_dir() if directory else resolved.is_file()
    if resolved != supplied or not correct_kind:
        kind = "directory" if directory else "file"
        raise RuntimeRequestError(f"{label} path must identify its canonical {kind}")
    return resolved
