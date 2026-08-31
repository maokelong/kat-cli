from __future__ import annotations

from dataclasses import dataclass
import json
import logging
from pathlib import Path
from typing import cast
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
    datasource_root: Path


@dataclass(frozen=True)
class QueryRunRequest:
    outputs: dict[str, Path]
    sql: str
    result_path: Path


@dataclass(frozen=True)
class TestPackRequest:
    pack_name: str
    pack_path: Path
    datasets: dict[str, ResolvedDatasetRef]
    tests: list[str]


type RuntimeRequest = (
    InspectPackRequest | RunWorkflowRequest | QueryRunRequest | TestPackRequest
)


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
    if operation == "query_run":
        return _read_query_run_request(request)
    if operation == "test_pack":
        return _read_test_pack_request(request)
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
        "datasource_root",
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
    pack_name = request["pack_name"]
    candidate = _read_run_candidate(
        request["candidate_id"],
        request["candidate_path"],
    )
    datasource_root = _read_creatable_directory(
        request["datasource_root"],
        "run_workflow Datasource root",
    )
    if (
        datasource_root.name != pack_name
        or datasource_root.parent.name != "datasources"
    ):
        raise RuntimeRequestError(
            "run_workflow Datasource root does not match the selected PACK"
        )
    data_home = datasource_root.parent.parent
    try:
        selected_runs_root = (data_home / "runs").resolve(strict=True)
    except (OSError, RuntimeError):
        _LOGGER.exception("failed to resolve the selected Data Home runs directory")
        raise RuntimeRequestError(
            "run_workflow Datasource root does not match the selected PACK"
        ) from None
    if candidate.path.parent != selected_runs_root:
        raise RuntimeRequestError(
            "run_workflow Datasource root does not match the selected PACK"
        )
    return RunWorkflowRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(request["pack_path"], "run_workflow PACK"),
        workflow_name=request["workflow_name"],
        dataset=dataset,
        arguments=arguments,
        candidate=candidate,
        datasource_root=datasource_root,
    )


def _read_query_run_request(request: dict[str, object]) -> QueryRunRequest:
    expected = {"operation", "outputs", "sql", "result_path"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"query_run Runtime Request fields must be exactly {sorted(expected)}"
        )
    outputs = request["outputs"]
    sql = request["sql"]
    if type(outputs) is not dict or type(sql) is not str:
        raise RuntimeRequestError(
            "query_run outputs must be an object and SQL must be a string"
        )
    resolved_outputs: dict[str, Path] = {}
    for name, path in outputs.items():
        if type(name) is not str or not valid_table_name(name) or type(path) is not str:
            raise RuntimeRequestError("query_run output references are invalid")
        resolved_outputs[name] = _canonical_file(path, "query_run output")
    return QueryRunRequest(
        outputs=resolved_outputs,
        sql=sql,
        result_path=_read_creatable_file(request["result_path"], "query_run result"),
    )


def _read_test_pack_request(request: dict[str, object]) -> TestPackRequest:
    datasets = cast(dict[str, dict[str, object]], request["datasets"])
    return TestPackRequest(
        pack_name=cast(str, request["pack_name"]),
        pack_path=Path(cast(str, request["pack_path"])),
        datasets={
            name: _construct_query_dataset(value) for name, value in datasets.items()
        },
        tests=cast(list[str], request["tests"]),
    )


def _construct_query_dataset(value: dict[str, object]) -> ResolvedDatasetRef:
    tables = cast(dict[str, str], value["tables"])
    return ResolvedDatasetRef(
        path=Path(cast(str, value["path"])),
        tables={name: Path(path) for name, path in tables.items()},
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


def _read_creatable_directory(value: object, label: str) -> Path:
    if type(value) is not str:
        raise RuntimeRequestError(f"{label} path must be a string")
    supplied = Path(value)
    if not supplied.is_absolute():
        raise RuntimeRequestError(f"{label} path must be absolute")
    try:
        resolved = supplied.resolve(strict=False)
    except (OSError, RuntimeError):
        _LOGGER.exception("failed to resolve private Runtime Request path for %s", label)
        raise RuntimeRequestError(f"{label} path is invalid") from None
    if resolved != supplied or supplied.is_symlink():
        raise RuntimeRequestError(f"{label} path must be canonical and must not be a link")
    if supplied.exists() and not supplied.is_dir():
        raise RuntimeRequestError(f"{label} path must identify a directory")
    return supplied


def _read_creatable_file(value: object, label: str) -> Path:
    if type(value) is not str:
        raise RuntimeRequestError(f"{label} path must be a string")
    supplied = Path(value)
    if not supplied.is_absolute():
        raise RuntimeRequestError(f"{label} path must be absolute")
    try:
        parent = supplied.parent.resolve(strict=True)
        resolved = supplied.resolve(strict=False)
    except (OSError, RuntimeError):
        _LOGGER.exception("failed to resolve private Runtime Request path for %s", label)
        raise RuntimeRequestError(f"{label} path is invalid") from None
    if (
        parent != supplied.parent
        or not parent.is_dir()
        or resolved != supplied
        or supplied.exists()
    ):
        raise RuntimeRequestError(
            f"{label} path must be a new canonical file in an existing directory"
        )
    return supplied


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
