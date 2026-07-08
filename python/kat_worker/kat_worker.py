from __future__ import annotations

import importlib
import inspect
import json
import sys
import traceback
from pathlib import Path
from typing import Any


def main() -> int:
    try:
        line = sys.stdin.readline()
        request = json.loads(line)
        if request.get("kind") != "run":
            write_message({"kind": "failed", "traceback": "first message must be run"})
            return 1
        return run_workflow(request)
    except Exception:
        write_message({"kind": "failed", "traceback": traceback.format_exc()})
        return 1


def run_workflow(request: dict[str, Any]) -> int:
    sdk_path = Path(request["sdkPath"])
    pack_root = Path(request["packRoot"])
    workflow_name = request["workflowName"]
    inputs = request.get("inputs", {})

    sys.path.insert(0, str(sdk_path))
    sys.path.insert(0, str(pack_root))

    import kat

    channel = JsonLineRuntimeChannel()
    kat.bind_runtime(channel)
    try:
        workflow_fn = discover_workflow(pack_root, workflow_name, kat)
        result = workflow_fn(**inputs)
        artifacts = kat.validate_workflow_return(result)
        write_message(
            {
                "kind": "complete",
                "artifacts": {
                    name: query_result.query_id
                    for name, query_result in artifacts.items()
                },
            }
        )
        return 0
    except Exception:
        write_message({"kind": "failed", "traceback": traceback.format_exc()})
        return 1
    finally:
        kat.reset_runtime()


class JsonLineRuntimeChannel:
    def query(self, sql: str, params: dict[str, Any]) -> dict[str, str]:
        response = self._request({"kind": "query", "sql": sql, "params": dict(params)})
        self._expect_kind(response, "queryResult")
        return {"queryId": str(response["queryId"])}

    def preview(self, query_id: str, limit: int) -> list[dict[str, Any]]:
        response = self._request(
            {"kind": "preview", "queryId": query_id, "limit": limit}
        )
        self._expect_kind(response, "rowsResult")
        return list(response.get("rows", []))

    def rows(self, query_id: str, max_rows: int) -> list[dict[str, Any]]:
        response = self._request(
            {"kind": "rows", "queryId": query_id, "maxRows": max_rows}
        )
        self._expect_kind(response, "rowsResult")
        return list(response.get("rows", []))

    def log(self, level: str, message: str, fields: dict[str, Any]) -> None:
        response = self._request(
            {
                "kind": "log",
                "level": level,
                "message": message,
                "fields": dict(fields),
            }
        )
        self._expect_kind(response, "logResult")

    def _request(self, payload: dict[str, Any]) -> dict[str, Any]:
        write_message(payload)
        response = read_message()
        if response.get("kind") == "failed":
            raise RuntimeError(response.get("traceback") or response.get("message"))
        return response

    @staticmethod
    def _expect_kind(response: dict[str, Any], kind: str) -> None:
        if response.get("kind") != kind:
            raise RuntimeError(f"expected {kind} response, got {response!r}")


def discover_workflow(pack_root: Path, workflow_name: str, kat: Any) -> Any:
    workflows: dict[str, Any] = {}

    for path in sorted(pack_root.rglob("*.py")):
        if any(part.startswith("__pycache__") for part in path.parts):
            continue
        if path.name == "__init__.py":
            continue
        module_name = path.relative_to(pack_root).with_suffix("").as_posix().replace("/", ".")
        module = importlib.import_module(module_name)
        for _, fn in inspect.getmembers(module, inspect.isfunction):
            if fn.__module__ != module.__name__:
                continue
            spec = kat.get_workflow_spec(fn)
            if spec is None:
                continue
            workflows[spec.name or module_name] = fn

    try:
        return workflows[workflow_name]
    except KeyError as exc:
        available = ", ".join(sorted(workflows)) or "<none>"
        raise RuntimeError(
            f"workflow {workflow_name!r} not found; available workflows: {available}"
        ) from exc


def read_message() -> dict[str, Any]:
    line = sys.stdin.readline()
    if not line:
        raise RuntimeError("runtime channel closed")
    return json.loads(line)


def write_message(message: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    raise SystemExit(main())
