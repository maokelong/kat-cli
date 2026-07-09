from __future__ import annotations

import argparse
import json
import traceback
from pathlib import Path
from typing import Any

from datafusion import SessionContext
from kat import Kat

from kat_runtime.artifacts import materialize_artifacts
from kat_runtime.dataset import register_dataset
from kat_runtime.manifest import now_iso, write_manifest
from kat_runtime.pack_loader import find_workflow


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    args = parser.parse_args()
    request_path = Path(args.request)
    request = json.loads(request_path.read_text(encoding="utf-8"))
    run_dir = Path(request["runDir"])
    started_at = now_iso()

    try:
        artifacts = run_request(request, run_dir)
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
        manifest = {
            "status": "failed",
            "packRoot": request.get("packRoot"),
            "workflow": request.get("workflow"),
            "datasetPath": request.get("datasetPath"),
            "error": {
                "kind": error.__class__.__name__,
                "message": str(error),
                "traceback": traceback.format_exc(),
            },
            "startedAt": started_at,
            "finishedAt": now_iso(),
        }
        write_manifest(run_dir, manifest)
        print(json.dumps({"status": "failed", "error": manifest["error"]}, ensure_ascii=False))
        return 1


def run_request(request: dict[str, Any], run_dir: Path) -> list[dict[str, Any]]:
    ctx = SessionContext()
    register_dataset(ctx, Path(request["datasetPath"]))
    workflow = find_workflow(Path(request["packRoot"]), request["workflow"])
    kat = Kat(ctx=ctx, run_dir=str(run_dir), logger=None)
    result = workflow(kat, **request.get("inputs", {}))
    return materialize_artifacts(result, run_dir)


if __name__ == "__main__":
    raise SystemExit(main())
