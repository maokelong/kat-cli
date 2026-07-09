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
