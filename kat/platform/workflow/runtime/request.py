from __future__ import annotations

from dataclasses import dataclass
import json
import logging
from pathlib import Path
from typing import cast
import uuid

from kat._identifiers import valid_source_name, valid_table_name


_LOGGER = logging.getLogger(__name__)


class RuntimeRequestError(Exception):
    """The control file does not contain a valid Runtime Request."""


@dataclass(frozen=True)
class InspectPackRequest:
    pack_name: str
    pack_path: Path


@dataclass(frozen=True)
class BindSourceRequest:
    pack_name: str
    pack_path: Path
    source_name: str
    arguments: tuple[str, ...]
    argument_base: Path


@dataclass(frozen=True)
class MaterializeSourceRequest:
    pack_name: str
    pack_path: Path
    source_name: str
    arguments: tuple[str, ...]
    argument_base: Path
    tables: tuple[str, ...]
    export_path: Path


@dataclass(frozen=True)
class ExternalSourceRef:
    pack: str
    source: str
    arguments: tuple[str, ...]
    working_directory: Path


@dataclass(frozen=True)
class ResolvedTableRef:
    name: str
    path: Path


@dataclass(frozen=True)
class MaterializedSourceRef:
    pack: str
    source: str
    tables: tuple[ResolvedTableRef, ...]


type ResolvedSourceRef = ExternalSourceRef | MaterializedSourceRef


@dataclass(frozen=True)
class PackSearchRef:
    candidates: dict[str, tuple[Path, ...]]
    issues: tuple[str, ...]


@dataclass(frozen=True)
class ResolvedDatasetRef:
    path: Path
    sources: tuple[ResolvedSourceRef, ...]

    def source(self, pack: str, source: str) -> ResolvedSourceRef | None:
        return next(
            (
                candidate
                for candidate in self.sources
                if candidate.pack == pack and candidate.source == source
            ),
            None,
        )


@dataclass(frozen=True)
class RunCandidateRef:
    identifier: str
    path: Path


@dataclass(frozen=True)
class RunWorkflowRequest:
    pack_name: str
    pack_path: Path
    pack_paths: dict[str, Path]
    workflow_name: str
    dataset: ResolvedDatasetRef | None
    arguments: list[str]
    candidate: RunCandidateRef


@dataclass(frozen=True)
class QueryRunRequest:
    run_path: Path
    outputs: tuple[str, ...]
    dataset: ResolvedDatasetRef | None
    pack_search: PackSearchRef
    sql: str


@dataclass(frozen=True)
class QueryDatasetRequest:
    dataset: ResolvedDatasetRef
    pack_search: PackSearchRef
    sql: str


@dataclass(frozen=True)
class TestPackRequest:
    pack_name: str
    pack_path: Path
    datasets: dict[str, ResolvedDatasetRef]
    tests: list[str]


type RuntimeRequest = (
    InspectPackRequest
    | BindSourceRequest
    | MaterializeSourceRequest
    | RunWorkflowRequest
    | QueryRunRequest
    | QueryDatasetRequest
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
    if operation == "inspect_pack":
        return _read_inspect_pack_request(request)
    if operation == "bind_source":
        return _read_bind_source_request(request)
    if operation == "materialize_source":
        return _read_materialize_source_request(request)
    if operation == "run_workflow":
        return _read_run_workflow_request(request)
    if operation == "query_run":
        return _read_query_run_request(request)
    if operation == "query_dataset":
        return _read_query_dataset_request(request)
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
        "pack_paths",
        "workflow_name",
        "arguments",
        "candidate_id",
        "candidate_path",
    }
    if set(request) not in (required, required | {"dataset"}):
        raise RuntimeRequestError("run_workflow Runtime Request has an invalid field set")
    strings = required - {"operation", "arguments", "pack_paths"}
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
        pack_paths=_read_pack_paths(request["pack_paths"], "run_workflow"),
        workflow_name=request["workflow_name"],
        dataset=dataset,
        arguments=arguments,
        candidate=_read_run_candidate(
            request["candidate_id"],
            request["candidate_path"],
        ),
    )


def _read_query_run_request(request: dict[str, object]) -> QueryRunRequest:
    required = {"operation", "run_path", "outputs", "pack_search", "sql"}
    if set(request) not in (required, required | {"dataset"}):
        raise RuntimeRequestError("query_run Runtime Request has an invalid field set")
    run_path = request["run_path"]
    outputs = request["outputs"]
    sql = request["sql"]
    if type(run_path) is not str or type(sql) is not str:
        raise RuntimeRequestError("query_run paths and SQL must be strings")
    if type(outputs) is not list or any(
        type(name) is not str or not valid_table_name(name) for name in outputs
    ):
        raise RuntimeRequestError("query_run outputs must contain valid names")
    if outputs != sorted(outputs) or len(outputs) != len(set(outputs)):
        raise RuntimeRequestError("query_run outputs must be uniquely sorted")
    dataset_value = cast(dict[str, object] | None, request.get("dataset"))
    return QueryRunRequest(
        run_path=_canonical_directory(run_path, "query_run Run"),
        outputs=tuple(outputs),
        dataset=(
            _read_resolved_dataset(dataset_value)
            if dataset_value is not None
            else None
        ),
        pack_search=_read_pack_search(request["pack_search"], "query_run"),
        sql=sql,
    )


def _read_bind_source_request(request: dict[str, object]) -> BindSourceRequest:
    common = _read_source_operation_request(request, "bind_source", extra=set())
    return BindSourceRequest(**common)


def _read_materialize_source_request(
    request: dict[str, object],
) -> MaterializeSourceRequest:
    common = _read_source_operation_request(
        request,
        "materialize_source",
        extra={"tables", "export_path"},
    )
    tables = request["tables"]
    export_path = request["export_path"]
    if type(tables) is not list or any(
        type(table) is not str or not valid_table_name(table) for table in tables
    ):
        raise RuntimeRequestError(
            "materialize_source tables must contain valid table names"
        )
    if tables != sorted(tables) or len(tables) != len(set(tables)):
        raise RuntimeRequestError(
            "materialize_source tables must have uniquely sorted names"
        )
    if type(export_path) is not str:
        raise RuntimeRequestError("materialize_source export_path must be a string")
    export = _canonical_directory(export_path, "materialize_source export")
    try:
        if next(export.iterdir(), None) is not None:
            raise RuntimeRequestError(
                "materialize_source export directory must be empty"
            )
    except OSError as error:
        raise RuntimeRequestError(
            "materialize_source export directory must be readable"
        ) from error
    return MaterializeSourceRequest(
        **common,
        tables=tuple(tables),
        export_path=export,
    )


def _read_source_operation_request(
    request: dict[str, object],
    operation: str,
    *,
    extra: set[str],
) -> dict[str, object]:
    expected = {
        "operation",
        "pack_name",
        "pack_path",
        "source_name",
        "arguments",
        "argument_base",
        *extra,
    }
    if set(request) != expected:
        raise RuntimeRequestError(
            f"{operation} Runtime Request has an invalid field set"
        )
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    source_name = request["source_name"]
    arguments = request["arguments"]
    argument_base = request["argument_base"]
    if (
        type(pack_name) is not str
        or not pack_name
        or type(pack_path) is not str
        or type(source_name) is not str
        or not valid_source_name(source_name)
        or type(arguments) is not list
        or any(type(argument) is not str for argument in arguments)
        or type(argument_base) is not str
    ):
        raise RuntimeRequestError(f"{operation} Runtime Request fields are invalid")
    base = Path(argument_base)
    if not base.is_absolute():
        raise RuntimeRequestError(f"{operation} argument_base must be absolute")
    return {
        "pack_name": pack_name,
        "pack_path": _canonical_directory(pack_path, f"{operation} PACK"),
        "source_name": source_name,
        "arguments": tuple(arguments),
        "argument_base": base,
    }


def _read_query_dataset_request(
    request: dict[str, object],
) -> QueryDatasetRequest:
    expected = {"operation", "dataset", "pack_search", "sql"}
    if set(request) != expected:
        raise RuntimeRequestError(
            "query_dataset Runtime Request has an invalid field set"
        )
    sql = request["sql"]
    if type(sql) is not str:
        raise RuntimeRequestError("query_dataset SQL must be a string")
    return QueryDatasetRequest(
        dataset=_read_resolved_dataset(request["dataset"]),
        pack_search=_read_pack_search(request["pack_search"], "query_dataset"),
        sql=sql,
    )


def _read_test_pack_request(request: dict[str, object]) -> TestPackRequest:
    expected = {"operation", "pack_name", "pack_path", "datasets", "tests"}
    if set(request) != expected:
        raise RuntimeRequestError("test_pack Runtime Request has an invalid field set")
    pack_name = request["pack_name"]
    pack_path = request["pack_path"]
    datasets = request["datasets"]
    tests = request["tests"]
    if (
        type(pack_name) is not str
        or not pack_name
        or type(pack_path) is not str
        or type(datasets) is not dict
        or type(tests) is not list
        or any(type(value) is not str for value in tests)
    ):
        raise RuntimeRequestError("test_pack Runtime Request fields are invalid")
    return TestPackRequest(
        pack_name=pack_name,
        pack_path=_canonical_directory(pack_path, "test_pack PACK"),
        datasets={
            name: _read_resolved_dataset(value)
            for name, value in _read_dataset_mapping(datasets).items()
        },
        tests=tests,
    )


def _read_dataset_mapping(
    value: dict[object, object],
) -> dict[str, object]:
    if any(type(name) is not str or not name for name in value):
        raise RuntimeRequestError("test_pack Dataset names must be non-empty strings")
    names = list(cast(dict[str, object], value))
    if names != sorted(names):
        raise RuntimeRequestError("test_pack Datasets must be sorted by name")
    return cast(dict[str, object], value)


def _read_resolved_dataset(value: object) -> ResolvedDatasetRef:
    if type(value) is not dict or set(value) != {"path", "sources"}:
        raise RuntimeRequestError(
            "Runtime Dataset must contain exactly path and sources"
        )
    path = value["path"]
    sources = value["sources"]
    if type(path) is not str or type(sources) is not list:
        raise RuntimeRequestError(
            "Runtime Dataset path and sources have invalid types"
        )
    root = _canonical_directory(path, "Runtime Dataset")
    resolved_sources = tuple(_read_resolved_source(item, root) for item in sources)
    identities = [(item.pack, item.source) for item in resolved_sources]
    if identities != sorted(identities) or len(identities) != len(set(identities)):
        raise RuntimeRequestError(
            "Runtime Dataset sources must be uniquely sorted by PACK and Source"
        )
    return ResolvedDatasetRef(path=root, sources=resolved_sources)


def _read_resolved_source(value: object, dataset: Path) -> ResolvedSourceRef:
    if type(value) is not dict:
        raise RuntimeRequestError("Runtime Dataset Source must be a JSON object")
    kind = value.get("kind")
    if kind == "external":
        expected = {
            "pack",
            "source",
            "kind",
            "arguments",
            "working_directory",
        }
        if set(value) != expected:
            raise RuntimeRequestError(
                "External Binding reference has an invalid field set"
            )
        pack, source = _read_source_identity(value)
        arguments = value["arguments"]
        working_directory = value["working_directory"]
        if type(arguments) is not list or any(
            type(argument) is not str for argument in arguments
        ):
            raise RuntimeRequestError(
                "External Binding arguments must be an array of strings"
            )
        if type(working_directory) is not str:
            raise RuntimeRequestError(
                "External Binding working_directory must be a string"
            )
        working_directory_path = Path(working_directory)
        if not working_directory_path.is_absolute():
            raise RuntimeRequestError(
                "External Binding working_directory must be absolute"
            )
        return ExternalSourceRef(
            pack=pack,
            source=source,
            arguments=tuple(arguments),
            working_directory=working_directory_path,
        )
    if kind == "materialized":
        expected = {"pack", "source", "kind", "tables"}
        if set(value) != expected:
            raise RuntimeRequestError(
                "Materialized Source reference has an invalid field set"
            )
        pack, source = _read_source_identity(value)
        tables = value["tables"]
        if type(tables) is not list or not tables:
            raise RuntimeRequestError(
                "Materialized Source tables must be a non-empty array"
            )
        resolved_tables = tuple(
            _read_resolved_table(table, dataset, pack, source) for table in tables
        )
        names = [table.name for table in resolved_tables]
        if names != sorted(names) or len(names) != len(set(names)):
            raise RuntimeRequestError(
                "Materialized Source tables must have uniquely sorted names"
            )
        return MaterializedSourceRef(
            pack=pack,
            source=source,
            tables=resolved_tables,
        )
    raise RuntimeRequestError("Runtime Dataset Source kind is unsupported")


def _read_source_identity(value: dict[str, object]) -> tuple[str, str]:
    pack = value["pack"]
    source = value["source"]
    if (
        type(pack) is not str
        or not _valid_pack_name(pack)
        or type(source) is not str
        or not valid_source_name(source)
    ):
        raise RuntimeRequestError("Runtime Dataset Source identity is invalid")
    return pack, source


def _read_resolved_table(
    value: object, dataset: Path, pack: str, source: str
) -> ResolvedTableRef:
    if type(value) is not dict or set(value) != {"name", "path"}:
        raise RuntimeRequestError(
            "Materialized Source table must contain exactly name and path"
        )
    name = value["name"]
    path = value["path"]
    if type(name) is not str or not valid_table_name(name) or type(path) is not str:
        raise RuntimeRequestError("Materialized Source table reference is invalid")
    resolved = _canonical_file(path, "Materialized Source table")
    table_root = dataset / "sources" / pack / source / "tables"
    expected_file = table_root / f"{name}.parquet"
    if resolved != expected_file:
        raise RuntimeRequestError(
            "Materialized Source table reference must remain in its Source space"
        )
    return ResolvedTableRef(name=name, path=resolved)


def _valid_pack_name(name: str) -> bool:
    windows_devices = {"con", "prn", "aux", "nul"} | {
        f"{prefix}{number}"
        for prefix in ("com", "lpt")
        for number in range(1, 10)
    }
    return name not in windows_devices and bool(name) and all(
        segment
        and segment.isascii()
        and all(character.islower() or character.isdigit() for character in segment)
        for segment in name.split("-")
    )


def _read_pack_paths(value: object, operation: str) -> dict[str, Path]:
    if type(value) is not dict:
        raise RuntimeRequestError(f"{operation} pack_paths must be a JSON object")
    paths: dict[str, Path] = {}
    for name, path in value.items():
        if type(name) is not str or not _valid_pack_name(name) or type(path) is not str:
            raise RuntimeRequestError(f"{operation} pack_paths entries are invalid")
        paths[name] = _canonical_directory(path, f"{operation} PACK {name}")
    if list(paths) != sorted(paths):
        raise RuntimeRequestError(f"{operation} pack_paths must be sorted by PACK name")
    return paths


def _read_pack_search(value: object, operation: str) -> PackSearchRef:
    if type(value) is not dict or set(value) != {"candidates", "issues"}:
        raise RuntimeRequestError(
            f"{operation} pack_search must contain exactly candidates and issues"
        )
    candidate_value = value["candidates"]
    issue_value = value["issues"]
    if type(candidate_value) is not dict:
        raise RuntimeRequestError(
            f"{operation} pack_search candidates must be a JSON object"
        )
    candidates: dict[str, tuple[Path, ...]] = {}
    for name, raw_paths in candidate_value.items():
        if (
            type(name) is not str
            or not _valid_pack_name(name)
            or type(raw_paths) is not list
            or not raw_paths
            or any(type(path) is not str for path in raw_paths)
        ):
            raise RuntimeRequestError(
                f"{operation} pack_search candidate entries are invalid"
            )
        paths = tuple(Path(path) for path in raw_paths)
        if any(not path.is_absolute() for path in paths) or len(paths) != len(set(paths)):
            raise RuntimeRequestError(
                f"{operation} pack_search candidate paths must be unique absolute paths"
            )
        candidates[name] = paths
    if list(candidates) != sorted(candidates):
        raise RuntimeRequestError(
            f"{operation} pack_search candidates must be sorted by PACK name"
        )
    if type(issue_value) is not list or any(
        type(issue) is not str or not issue for issue in issue_value
    ):
        raise RuntimeRequestError(
            f"{operation} pack_search issues must be an array of non-empty strings"
        )
    return PackSearchRef(candidates=candidates, issues=tuple(issue_value))


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
