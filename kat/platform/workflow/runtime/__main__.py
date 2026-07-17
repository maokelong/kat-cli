from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import NoReturn

from .execution import run_workflow
from .pack import inspect_pack
from .query import DatasetCapabilityError, query_run
from .testing import PytestExitError, test_pack


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--request", required=True)
    parser.add_argument("--response", required=True)
    parser.add_argument("--test-report")
    arguments = parser.parse_args()
    response_path = Path(arguments.response)
    pack_path: Path | None = None
    operation = "Runtime"
    private_values: tuple[str, ...] = ()
    try:
        request = _load_request(Path(arguments.request))
        requested_operation = request.get("operation")
        if type(requested_operation) is str:
            operation = requested_operation
        _validate_request(request)
        operation = request["operation"]
        test_report_path = _test_report_path(arguments.test_report, operation)
        if operation == "inspect_pack":
            pack_path = Path(request["pack_path"])
            result = inspect_pack(request["pack_name"], request["pack_path"])
        elif operation == "run_workflow":
            pack_path = Path(request["pack_path"])
            private_values = (request["candidate_id"], request["run_path"])
            result = run_workflow(request)
        elif operation == "query_run":
            private_values = (request["run_path"], *request["outputs"].values())
            result = query_run(request)
        elif operation == "test_pack":
            pack_path = Path(request["pack_path"])
            private_values = (
                request["pack_path"],
                str(test_report_path),
                *(
                    dataset["path"]
                    for dataset in request["datasets"].values()
                ),
            )
            result = test_pack(request, test_report_path)
        else:
            raise ValueError("unsupported Runtime Request operation")
        response: dict[str, object] = {"status": "success", "result": result}
    except Exception as error:
        response = {
            "status": "failure",
            "error": _diagnostic(error, pack_path, operation, private_values),
        }
    _write_response(response_path, response)
    return 0


def _load_request(path: Path) -> dict[str, object]:
    with path.open("r", encoding="utf-8") as file:
        request = json.load(file)
    if type(request) is not dict:
        raise ValueError("Runtime Request must be a JSON object")

    return request


def _validate_request(request: dict[str, object]) -> None:
    operation = request.get("operation")
    if operation == "inspect_pack":
        expected = {"operation", "pack_name", "pack_path"}
        if set(request) != expected:
            raise ValueError(
                f"inspect_pack Runtime Request fields must be exactly {sorted(expected)}"
            )
        if type(request["pack_name"]) is not str or type(request["pack_path"]) is not str:
            raise TypeError("inspect_pack Runtime Request fields must be strings")
    elif operation == "run_workflow":
        required = {
            "operation",
            "pack_name",
            "pack_path",
            "workflow_name",
            "arguments",
            "candidate_id",
            "run_path",
        }
        if set(request) not in (required, required | {"dataset"}):
            raise ValueError("run_workflow Runtime Request has an invalid field set")
        if any(type(request[name]) is not str for name in required - {"operation", "arguments"}):
            raise TypeError("run_workflow identity and path fields must be strings")
        if type(request["arguments"]) is not list or any(
            type(value) is not str for value in request["arguments"]
        ):
            raise TypeError("run_workflow arguments must be an array of strings")
        if "dataset" in request:
            dataset = request["dataset"]
            if type(dataset) is not dict or set(dataset) != {"path", "tables"}:
                raise ValueError("run_workflow Dataset must contain exactly path and tables")
            if type(dataset["path"]) is not str or type(dataset["tables"]) is not dict:
                raise TypeError("run_workflow Dataset path and tables have invalid types")
            if any(
                type(name) is not str or type(value) is not str
                for name, value in dataset["tables"].items()
            ):
                raise TypeError("run_workflow Dataset table references must be strings")
    elif operation == "query_run":
        expected = {"operation", "run_id", "run_path", "outputs", "dataset", "sql"}
        if set(request) != expected:
            raise ValueError(
                f"query_run Runtime Request fields must be exactly {sorted(expected)}"
            )
        if any(type(request[name]) is not str for name in ("run_id", "run_path", "sql")):
            raise TypeError("query_run identity, path, and SQL fields must be strings")
        outputs = request["outputs"]
        if type(outputs) is not dict or not outputs or any(
            type(name) is not str or type(value) is not str
            for name, value in outputs.items()
        ):
            raise TypeError("query_run outputs must be a non-empty string mapping")
        dataset = request["dataset"]
        if type(dataset) is not dict:
            raise TypeError("query_run dataset must be an object")
    elif operation == "test_pack":
        expected = {"operation", "pack_name", "pack_path", "datasets", "tests"}
        if set(request) != expected:
            raise ValueError(
                f"test_pack Runtime Request fields must be exactly {sorted(expected)}"
            )
        if any(type(request[name]) is not str or not request[name] for name in ("pack_name", "pack_path")):
            raise TypeError("test_pack PACK identity and path must be non-empty strings")
        datasets = request["datasets"]
        if type(datasets) is not dict or any(
            type(name) is not str or not name or type(dataset) is not dict
            for name, dataset in datasets.items()
        ):
            raise TypeError("test_pack datasets must be a named object mapping")
        for dataset in datasets.values():
            if set(dataset) != {"path", "tables"}:
                raise ValueError("test_pack Dataset must contain exactly path and tables")
            if type(dataset["path"]) is not str or type(dataset["tables"]) is not dict:
                raise TypeError("test_pack Dataset path and tables have invalid types")
            if any(
                type(name) is not str or type(path) is not str
                for name, path in dataset["tables"].items()
            ):
                raise TypeError("test_pack Dataset table references must be strings")
        tests = request["tests"]
        if type(tests) is not list or any(type(test) is not str for test in tests):
            raise TypeError("test_pack tests must be an array of strings")
    else:
        raise ValueError("unsupported Runtime Request operation")


def _diagnostic(
    error: Exception,
    pack_path: Path | None,
    operation: str,
    private_values: tuple[str, ...],
) -> dict[str, object]:
    causes: list[str] = []
    current: BaseException | None = error
    while current is not None:
        rendered = str(current).strip()
        if rendered:
            for private in private_values:
                rendered = rendered.replace(private, "<private>")
            causes.append(rendered)
        current = current.__cause__ or current.__context__
    if operation == "run_workflow":
        diagnostic: dict[str, object] = {
            "message": "Workflow execution failed",
            "help": "Correct the Workflow, arguments, or Dataset and retry the complete Run",
        }
    elif operation == "query_run":
        if isinstance(error, DatasetCapabilityError):
            diagnostic = {
                "message": "Run Output query requires the current Dataset",
                "help": error.help(),
            }
        else:
            diagnostic = {
                "message": "Run Output query failed",
                "help": "Narrow the projection, filter, aggregate, or use an explicit LIMIT, then retry",
            }
    elif operation == "test_pack":
        if isinstance(error, PytestExitError):
            diagnostic = {"message": error.message(), "help": error.help()}
        else:
            diagnostic = {
                "message": "PACK test Runtime failed",
                "help": "Inspect the pytest terminal report and Operation log, correct the PACK, and retry",
            }
    else:
        diagnostic = {
            "message": "PACK inspection failed",
            "help": "Correct the PACK production Interface and retry inspection",
        }
    if causes:
        diagnostic["causes"] = causes
    location = _syntax_error_location(error, pack_path)
    if location is not None:
        diagnostic["location"] = location
    return diagnostic


def _test_report_path(value: str | None, operation: str) -> Path | None:
    if operation != "test_pack":
        if value is not None:
            raise ValueError("--test-report is valid only for test_pack")
        return None
    if value is None:
        raise ValueError("test_pack requires --test-report")
    path = Path(value)
    if not path.is_absolute() or path.exists():
        raise ValueError("test_pack report path must be a new absolute path")
    parent = path.parent.resolve(strict=True)
    if parent != path.parent or not parent.is_dir():
        raise ValueError("test_pack report parent must be a canonical directory")
    return path


def _syntax_error_location(
    error: Exception, pack_path: Path | None
) -> dict[str, object] | None:
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


def _write_response(path: Path, response: dict[str, object]) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(response, file, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


if __name__ == "__main__":
    raise SystemExit(main())
