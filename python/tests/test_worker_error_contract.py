import json
import sys
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
sys.path[:0] = [str(SDK_ROOT), str(RUNTIME_ROOT)]

import kat_runtime.worker.run as worker


def write_request(tmp_path: Path, *, inputs=None) -> tuple[Path, Path]:
    run_dir = tmp_path / "run"
    request_path = tmp_path / "request.json"
    request_path.write_text(
        json.dumps(
            {
                "packRoot": str(tmp_path / "pack"),
                "workflow": "sample",
                "datasetPath": str(tmp_path / "dataset"),
                "runDir": str(run_dir),
                "inputs": {} if inputs is None else inputs,
            }
        ),
        encoding="utf-8",
    )
    return request_path, run_dir


def read_manifest(run_dir: Path) -> dict:
    return json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))


def patch_success_path(monkeypatch, workflow_function):
    monkeypatch.setattr(worker, "SessionContext", lambda: object())
    monkeypatch.setattr(worker, "register_dataset", lambda ctx, path: None)
    monkeypatch.setattr(worker, "load_pack_modules", lambda path: [object()])
    monkeypatch.setattr(
        worker,
        "find_workflow",
        lambda modules, workflow_name: workflow_function,
    )
    monkeypatch.setattr(worker, "validate_artifacts", lambda result, run_dir: ["plan"])
    monkeypatch.setattr(
        worker,
        "materialize_artifacts",
        lambda plans: [{"name": "path_nodes", "path": "artifacts/path_nodes.parquet"}],
    )


def assert_failure(run_dir: Path, expected_kind: str, expected_type: str) -> None:
    manifest = read_manifest(run_dir)
    assert manifest["status"] == "failed"
    assert manifest["error"]["kind"] == expected_kind
    assert manifest["error"]["type"] == expected_type
    assert manifest["error"]["message"]
    assert "Traceback" in manifest["error"]["traceback"]


def test_request_contract_uses_request_parent_as_manifest_fallback(tmp_path):
    request_path = tmp_path / "request.json"
    request_path.write_text("{", encoding="utf-8")

    assert worker.run_request_file(request_path) == 1

    assert_failure(tmp_path, "request_contract", "JSONDecodeError")


@pytest.mark.parametrize(
    "payload",
    [
        [],
        {
            "packRoot": "pack",
            "workflow": "sample",
            "datasetPath": "dataset",
            "inputs": {},
        },
        {
            "packRoot": "pack",
            "workflow": "sample",
            "datasetPath": "dataset",
            "runDir": 42,
            "inputs": {},
        },
    ],
)
def test_request_contract_rejects_shape_and_invalid_run_dir(tmp_path, payload):
    request_path = tmp_path / "request.json"
    request_path.write_text(json.dumps(payload), encoding="utf-8")

    assert worker.run_request_file(request_path) == 1

    manifest = read_manifest(tmp_path)
    assert manifest["status"] == "failed"
    assert manifest["error"]["kind"] == "request_contract"


@pytest.mark.parametrize(
    "kind",
    [
        "session_creation",
        "dataset_registration",
        "pack_load",
        "workflow_selection",
        "input_contract",
        "workflow_execution",
        "return_contract",
        "materialization",
    ],
)
def test_worker_reports_stable_phase_kind(monkeypatch, tmp_path, kind):
    request_path, run_dir = write_request(tmp_path)

    def successful_workflow(kat):
        return {"path_nodes": object()}

    patch_success_path(monkeypatch, successful_workflow)
    expected_type = "RuntimeError"

    if kind == "session_creation":
        monkeypatch.setattr(
            worker,
            "SessionContext",
            lambda: (_ for _ in ()).throw(RuntimeError("session boom")),
        )
    elif kind == "dataset_registration":
        monkeypatch.setattr(
            worker,
            "register_dataset",
            lambda ctx, path: (_ for _ in ()).throw(ValueError("dataset boom")),
        )
        expected_type = "ValueError"
    elif kind == "pack_load":
        monkeypatch.setattr(
            worker,
            "load_pack_modules",
            lambda path: (_ for _ in ()).throw(ImportError("import boom")),
        )
        expected_type = "ImportError"
    elif kind == "workflow_selection":
        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: (_ for _ in ()).throw(KeyError("missing workflow")),
        )
        expected_type = "KeyError"
    elif kind == "input_contract":
        def requires_input(kat, required):
            raise AssertionError("workflow body must not run")

        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: requires_input,
        )
        expected_type = "TypeError"
    elif kind == "workflow_execution":
        def failing_workflow(kat):
            raise TypeError("body boom")

        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: failing_workflow,
        )
        expected_type = "TypeError"
    elif kind == "return_contract":
        monkeypatch.setattr(
            worker,
            "validate_artifacts",
            lambda result, run_dir: (_ for _ in ()).throw(
                TypeError("return boom")
            ),
        )
        expected_type = "TypeError"
    elif kind == "materialization":
        monkeypatch.setattr(
            worker,
            "materialize_artifacts",
            lambda plans: (_ for _ in ()).throw(OSError("write boom")),
        )
        expected_type = "OSError"

    assert worker.run_request_file(request_path) == 1
    assert_failure(run_dir, kind, expected_type)


def test_workflow_selection_rejects_duplicate_names(tmp_path):
    from kat_runtime.pack_loader import find_workflow, load_pack_modules

    pack = tmp_path / "pack"
    pack.mkdir()
    for module_name in ["one.py", "two.py"]:
        (pack / module_name).write_text(
            """
from kat import workflow

@workflow(title="Duplicate", description="Duplicate name")
def duplicate(kat):
    return {}
""",
            encoding="utf-8",
        )

    modules = load_pack_modules(pack)
    with pytest.raises(ValueError, match="workflow is ambiguous: duplicate"):
        find_workflow(modules, "duplicate")


def test_ambiguous_workflow_is_reported_as_workflow_selection(
    monkeypatch, tmp_path
):
    request_path, run_dir = write_request(tmp_path)

    def successful_workflow(kat):
        return {}

    patch_success_path(monkeypatch, successful_workflow)
    monkeypatch.setattr(
        worker,
        "find_workflow",
        lambda modules, name: (_ for _ in ()).throw(
            ValueError("workflow is ambiguous: sample")
        ),
    )

    assert worker.run_request_file(request_path) == 1
    assert_failure(run_dir, "workflow_selection", "ValueError")
