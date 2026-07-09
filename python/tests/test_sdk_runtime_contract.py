import json
import os
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"


def pythonpath() -> str:
    return f"{SDK_ROOT}{';' if sys.platform == 'win32' else ':'}{RUNTIME_ROOT}"


def worker_env() -> dict[str, str]:
    env = os.environ.copy()
    env["PYTHONPATH"] = pythonpath()
    return env


def test_decorators_attach_metadata():
    sys.path.insert(0, str(SDK_ROOT))
    from kat import compute, fact, workflow

    @workflow(title="Workflow title", description="Workflow description")
    def sample_workflow(kat, root_itid: int):
        return {}

    @fact(title="Fact title", description="Fact description")
    def sample_fact(kat):
        return None

    @compute(title="Compute title", description="Compute description")
    def sample_compute(df):
        return df

    assert sample_workflow.__kat_capability__["kind"] == "workflow"
    assert sample_fact.__kat_capability__["kind"] == "fact"
    assert sample_compute.__kat_capability__["kind"] == "compute"


def test_discovery_worker_lists_pack_capabilities(tmp_path):
    pack_root = tmp_path / "sample_pack"
    pack_root.mkdir()
    (pack_root / "pack.py").write_text(
        """
from kat import compute, fact, workflow

@workflow(title="Hello", description="Run hello workflow")
def hello(kat, root_itid: int = 405):
    return {}

@fact(title="Threads", description="Read thread facts")
def threads(kat):
    return kat.sql("select * from thread")

@compute(title="Limit", description="Limit rows")
def limit(df, count: int = 10):
    return df.limit(count)
""",
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "kat_runtime.worker.discovery",
            "--pack-root",
            str(pack_root),
        ],
        env=worker_env(),
        text=True,
        capture_output=True,
        check=True,
    )
    manifest = json.loads(result.stdout)

    assert manifest["workflows"][0]["name"] == "hello"
    assert manifest["workflows"][0]["title"] == "Hello"
    assert "root_itid" in manifest["workflows"][0]["signature"]
    assert manifest["facts"][0]["name"] == "threads"
    assert manifest["computes"][0]["name"] == "limit"


def test_discovery_worker_ignores_reexported_capabilities(tmp_path):
    pack_root = tmp_path / "sample_pack"
    pack_root.mkdir()
    (pack_root / "defs.py").write_text(
        """
from kat import workflow


@workflow(title="Shared", description="Defined once")
def shared_workflow(kat, root_itid: int = 1):
    return {}
""",
        encoding="utf-8",
    )
    (pack_root / "pack.py").write_text(
        """
from defs import shared_workflow
""",
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "kat_runtime.worker.discovery",
            "--pack-root",
            str(pack_root),
        ],
        env=worker_env(),
        text=True,
        capture_output=True,
        check=True,
    )
    manifest = json.loads(result.stdout)

    assert len(manifest["workflows"]) == 1
    assert manifest["workflows"][0]["name"] == "shared_workflow"


def test_sql_binding_replaces_overlapping_parameters_atomically():
    sys.path.insert(0, str(SDK_ROOT))
    from kat.context import _bind_sql_params

    rendered = _bind_sql_params(
        "select :id as id, :id2 as id2",
        {"id": 1, "id2": 2},
    )

    assert rendered == "select 1 as id, 2 as id2"


def test_run_worker_materializes_returned_dataframes(tmp_path):
    dataset_path = tmp_path / "dataset"
    dataset_path.mkdir()
    tables_path = dataset_path / "tables"
    tables_path.mkdir()

    import datafusion

    ctx = datafusion.SessionContext()
    ctx.sql("select 405 as itid, 'main' as thread_name").write_parquet(
        str(tables_path / "thread.parquet")
    )
    (dataset_path / "catalog.json").write_text(
        json.dumps(
            {
                "tables": [
                    {
                        "name": "thread",
                        "path": "tables/thread.parquet",
                        "kind": "source",
                    }
                ]
            }
        ),
        encoding="utf-8",
    )

    pack_root = tmp_path / "pack"
    pack_root.mkdir()
    (pack_root / "pack.py").write_text(
        """
from kat import workflow

@workflow(title="Thread nodes", description="Return path nodes")
def thread_nodes(kat):
    nodes = kat.sql("select itid, thread_name from thread")
    edges = kat.sql("select cast(null as bigint) as from_itid, cast(null as bigint) as to_itid where false")
    return {"path_nodes": nodes, "path_edges": edges}
""",
        encoding="utf-8",
    )

    run_dir = tmp_path / "run"
    request = {
        "packRoot": str(pack_root),
        "workflow": "thread_nodes",
        "datasetPath": str(dataset_path),
        "runDir": str(run_dir),
        "inputs": {},
    }
    request_path = tmp_path / "request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "kat_runtime.worker.run",
            "--request",
            str(request_path),
        ],
        env=worker_env(),
        text=True,
        capture_output=True,
        check=True,
    )

    manifest = json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["status"] == "success"
    assert (run_dir / "artifacts" / "path_nodes.parquet").exists()
    assert (run_dir / "artifacts" / "path_edges.parquet").exists()
    assert "path_nodes" in result.stdout


def test_openharmony_critical_path_pack_is_discoverable():
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "kat_runtime.worker.discovery",
            "--pack-root",
            str(REPO_ROOT / "packs" / "openharmony-critical-path"),
        ],
        env=worker_env(),
        text=True,
        capture_output=True,
        check=True,
    )
    manifest = json.loads(result.stdout)
    workflow_names = {item["name"] for item in manifest["workflows"]}
    compute_names = {item["name"] for item in manifest["computes"]}

    assert "wechat_first_frame_critical_path" in workflow_names
    assert "critical_path" in compute_names
