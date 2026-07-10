from __future__ import annotations

import argparse
import inspect
import json
import sys
import traceback
from pathlib import Path
from typing import Any

from datafusion import SessionContext
from kat import Kat

from kat_runtime.artifacts import materialize_artifacts, validate_artifacts
from kat_runtime.dataset import register_dataset
from kat_runtime.manifest import now_iso, write_manifest
from kat_runtime.pack_loader import find_workflow, load_pack_modules


REQUIRED_STRING_FIELDS = ("packRoot", "workflow", "datasetPath", "runDir")


def _read_request(request_path: Path) -> dict[str, Any]:
    request = json.loads(request_path.read_text(encoding="utf-8"))
    if not isinstance(request, dict):
        raise TypeError("request must be a JSON object")
    return request


def _validate_request(request: dict[str, Any]) -> None:
    for field in REQUIRED_STRING_FIELDS:
        value = request.get(field)
        if not isinstance(value, str) or not value:
            raise ValueError(f"request field {field} must be a non-empty string")
    inputs = request.get("inputs", {})
    if not isinstance(inputs, dict):
        raise TypeError("request field inputs must be an object")
    request["inputs"] = inputs


def _failure_manifest(
    request: dict[str, Any],
    *,
    kind: str,
    error: Exception,
    started_at: str,
) -> dict[str, Any]:
    return {
        "status": "failed",
        "packRoot": request.get("packRoot"),
        "workflow": request.get("workflow"),
        "datasetPath": request.get("datasetPath"),
        "error": {
            "kind": kind,
            "type": error.__class__.__name__,
            "message": str(error),
            "traceback": traceback.format_exc(),
        },
        "startedAt": started_at,
        "finishedAt": now_iso(),
    }


def run_request_file(request_path: Path) -> int:
    fallback_run_dir = request_path.parent
    run_dir = fallback_run_dir
    request: dict[str, Any] = {}
    started_at = now_iso()
    phase = "request_contract"

    try:
        request = _read_request(request_path)
        requested_run_dir = request.get("runDir")
        if isinstance(requested_run_dir, str) and requested_run_dir:
            run_dir = Path(requested_run_dir)
        _validate_request(request)

        phase = "session_creation"
        ctx = SessionContext()

        phase = "dataset_registration"
        register_dataset(ctx, Path(request["datasetPath"]))

        phase = "pack_load"
        modules = load_pack_modules(Path(request["packRoot"]))

        phase = "workflow_selection"
        workflow = find_workflow(modules, request["workflow"])
        kat = Kat(ctx=ctx, run_dir=str(run_dir), logger=None)

        phase = "input_contract"
        inspect.signature(workflow).bind(kat, **request["inputs"])

        phase = "workflow_execution"
        result = workflow(kat, **request["inputs"])

        phase = "return_contract"
        plans = validate_artifacts(result, run_dir)

        phase = "materialization"
        artifacts = materialize_artifacts(plans)

        manifest = {
            "status": "success",
            "packRoot": request["packRoot"],
            "workflow": request["workflow"],
            "datasetPath": request["datasetPath"],
            "artifacts": artifacts,
            "startedAt": started_at,
            "finishedAt": now_iso(),
        }
        write_manifest(run_dir, manifest)
        print(json.dumps({"status": "success", "artifacts": artifacts}, ensure_ascii=False))
        return 0
    except Exception as error:
        manifest = _failure_manifest(
            request,
            kind=phase,
            error=error,
            started_at=started_at,
        )
        write_error = None
        for manifest_dir in dict.fromkeys([run_dir, fallback_run_dir]):
            try:
                write_manifest(manifest_dir, manifest)
                write_error = None
                break
            except Exception as candidate_error:
                write_error = candidate_error
        if write_error is not None:
            print(f"failed to write failure manifest: {write_error}", file=sys.stderr)
        print(json.dumps({"status": "failed", "error": manifest["error"]}, ensure_ascii=False))
        return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    args = parser.parse_args()
    return run_request_file(Path(args.request))


if __name__ == "__main__":
    raise SystemExit(main())
