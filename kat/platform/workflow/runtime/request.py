from __future__ import annotations

from dataclasses import dataclass
import json
import logging
from pathlib import Path
import uuid

from kat._identifiers import valid_table_name

from .rpc import _decode_inputs


_LOGGER = logging.getLogger(__name__)


class RuntimeRequestError(Exception):
    """The control file does not contain a valid Runtime Request."""


@dataclass(frozen=True)
class InspectWorkflowRequest:
    pack_name: str
    pack_path: Path
    workflow_name: str | None


@dataclass(frozen=True)
class InspectProviderRequest:
    pack_name: str
    pack_path: Path
    provider_name: str | None


@dataclass(frozen=True)
class RunCandidateRef:
    identifier: str
    path: Path


@dataclass(frozen=True)
class RunWorkflowRequest:
    pack_name: str
    pack_path: Path
    workflow_name: str
    arguments: list[str] | None
    inputs: dict[str, object] | None
    candidate: RunCandidateRef
    datasource_root: Path
    scratch_root: Path


@dataclass(frozen=True)
class QueryRunRequest:
    outputs: dict[str, Path]
    sql: str
    result_path: Path


@dataclass(frozen=True)
class TestPackRequest:
    pack_name: str
    pack_path: Path
    tests: list[str]


type RuntimeRequest = (
    InspectWorkflowRequest
    | InspectProviderRequest
    | RunWorkflowRequest
    | QueryRunRequest
    | TestPackRequest
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
    if operation == "inspect_workflow":
        return _read_inspect_workflow_request(request)
    if operation == "inspect_provider":
        return _read_inspect_provider_request(request)
    if operation == "run_workflow":
        return _read_run_workflow_request(request, with_inputs=False)
    if operation == "run_workflow_with_inputs":
        return _read_run_workflow_request(request, with_inputs=True)
    if operation == "query_run":
        return _read_query_run_request(request)
    if operation == "test_pack":
        return _read_test_pack_request(request)
    raise RuntimeRequestError("unsupported Runtime Request operation")


def _read_inspect_workflow_request(
    request: dict[str, object],
) -> InspectWorkflowRequest:
    expected = {"operation", "pack_name", "pack_path", "workflow_name"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"inspect_workflow Runtime Request fields must be exactly {sorted(expected)}"
        )
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    workflow_name = request["workflow_name"]
    if type(pack_name) is not str or type(pack_path) is not str:
        raise RuntimeRequestError(
            "inspect_workflow PACK name and path fields must be strings"
        )
    if not pack_name:
        raise RuntimeRequestError(
            "inspect_workflow Runtime Request PACK name must not be empty"
        )
    if workflow_name is not None and (
        type(workflow_name) is not str or not workflow_name
    ):
        raise RuntimeRequestError(
            "inspect_workflow workflow_name must be null or a non-empty string"
        )
    return InspectWorkflowRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(pack_path, "inspect_workflow PACK"),
        workflow_name=workflow_name,
    )


def _read_inspect_provider_request(
    request: dict[str, object],
) -> InspectProviderRequest:
    expected = {"operation", "pack_name", "pack_path", "provider_name"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"inspect_provider Runtime Request fields must be exactly {sorted(expected)}"
        )
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    provider_name = request["provider_name"]
    if type(pack_name) is not str or type(pack_path) is not str:
        raise RuntimeRequestError(
            "inspect_provider PACK name and path fields must be strings"
        )
    if not pack_name:
        raise RuntimeRequestError(
            "inspect_provider Runtime Request PACK name must not be empty"
        )
    if provider_name is not None and (
        type(provider_name) is not str or not provider_name
    ):
        raise RuntimeRequestError(
            "inspect_provider provider_name must be null or a non-empty string"
        )
    return InspectProviderRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(pack_path, "inspect_provider PACK"),
        provider_name=provider_name,
    )


def _read_run_workflow_request(
    request: dict[str, object], *, with_inputs: bool
) -> RunWorkflowRequest:
    common = {
        "operation",
        "pack_name",
        "pack_path",
        "workflow_name",
        "candidate_id",
        "candidate_path",
        "datasource_root",
        "scratch_root",
    }
    input_field = "inputs" if with_inputs else "arguments"
    required = common | {input_field}
    if set(request) != required:
        operation = "run_workflow_with_inputs" if with_inputs else "run_workflow"
        raise RuntimeRequestError(f"{operation} Runtime Request has an invalid field set")
    string_fields = {name: request[name] for name in common - {"operation"}}
    if any(type(value) is not str or not value for value in string_fields.values()):
        raise RuntimeRequestError(
            "run_workflow identity and path fields must be non-empty strings"
        )
    pack_name = string_fields["pack_name"]
    pack_path = string_fields["pack_path"]
    workflow_name = string_fields["workflow_name"]
    candidate_id = string_fields["candidate_id"]
    candidate_path = string_fields["candidate_path"]
    datasource_path = string_fields["datasource_root"]
    scratch_path = string_fields["scratch_root"]
    assert type(pack_name) is str
    assert type(pack_path) is str
    assert type(workflow_name) is str
    assert type(candidate_id) is str
    assert type(candidate_path) is str
    assert type(datasource_path) is str
    assert type(scratch_path) is str
    arguments: list[str] | None = None
    decoded_inputs: dict[str, object] | None = None
    if with_inputs:
        try:
            decoded_inputs = _decode_inputs(request["inputs"])
        except (TypeError, ValueError) as error:
            raise RuntimeRequestError(
                "run_workflow_with_inputs inputs are invalid"
            ) from error
    else:
        value = request["arguments"]
        if type(value) is not list or any(type(item) is not str for item in value):
            raise RuntimeRequestError(
                "run_workflow arguments must be an array of strings"
            )
        arguments = value
    candidate = _read_run_candidate(
        candidate_id,
        candidate_path,
    )
    datasource_root = _read_creatable_directory(
        datasource_path,
        "run_workflow Datasource root",
    )
    scratch_root = _read_creatable_directory(
        scratch_path,
        "run_workflow Scratch root",
    )
    session_root = candidate.path.parent.parent
    if (
        session_root.parent.name != "sessions"
        or not _is_canonical_uuid7(session_root.name)
        or candidate.path.parent.name != "runs"
        or datasource_root.name != "materializations"
        or datasource_root.parent != session_root
        or scratch_root.name != candidate.identifier
        or scratch_root.parent.name != "scratch"
        or scratch_root.parent.parent != session_root
    ):
        raise RuntimeRequestError("run_workflow paths do not match one Session")
    return RunWorkflowRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(pack_path, "run_workflow PACK"),
        workflow_name=workflow_name,
        arguments=arguments,
        inputs=decoded_inputs,
        candidate=candidate,
        datasource_root=datasource_root,
        scratch_root=scratch_root,
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
    expected = {"operation", "pack_name", "pack_path", "tests"}
    if set(request) != expected:
        raise RuntimeRequestError(
            f"test_pack Runtime Request fields must be exactly {sorted(expected)}"
        )
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    tests = request["tests"]
    if (
        type(pack_name) is not str
        or not pack_name
        or type(pack_path) is not str
        or type(tests) is not list
        or any(type(test) is not str for test in tests)
    ):
        raise RuntimeRequestError("test_pack Runtime Request fields have invalid types")
    return TestPackRequest(
        pack_name=pack_name,
        pack_path=Path(pack_path),
        tests=tests,
    )


def _read_run_candidate(candidate_id: str, candidate_path: str) -> RunCandidateRef:
    if not _is_canonical_uuid7(candidate_id):
        raise RuntimeRequestError("run_workflow candidate identity is invalid")
    path = _canonical_directory(candidate_path, "run_workflow candidate")
    if path.name != candidate_id or (path / "manifest.json").exists():
        raise RuntimeRequestError(
            "run_workflow candidate identity and directory do not match"
        )
    return RunCandidateRef(identifier=candidate_id, path=path)


def _is_canonical_uuid7(value: str) -> bool:
    try:
        identity = uuid.UUID(value)
    except ValueError:
        return False
    return identity.version == 7 and str(identity) == value


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
