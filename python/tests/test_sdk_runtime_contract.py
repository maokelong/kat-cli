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
