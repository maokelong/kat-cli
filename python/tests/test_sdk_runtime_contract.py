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


def test_critical_path_clips_segments_and_keeps_edges_consistent(tmp_path):
    dataset_path = tmp_path / "dataset"
    tables_path = dataset_path / "tables"
    tables_path.mkdir(parents=True)

    import datafusion

    ctx = datafusion.SessionContext()
    ctx.sql(
        """
        select cast(1 as bigint) as itid, cast(-100000 as bigint) as ts, cast(1200000 as bigint) as dur, cast('S' as varchar) as state
        union all
        select cast(2 as bigint) as itid, cast(100000 as bigint) as ts, cast(300000 as bigint) as dur, cast('Running' as varchar) as state
        union all
        select cast(3 as bigint) as itid, cast(799950 as bigint) as ts, cast(400000 as bigint) as dur, cast('Running' as varchar) as state
        union all
        select cast(4 as bigint) as itid, cast(900000 as bigint) as ts, cast(300000 as bigint) as dur, cast('Running' as varchar) as state
        """
    ).write_parquet(str(tables_path / "thread_state.parquet"))
    ctx.sql(
        """
        select cast(1 as bigint) as ref, cast('itid' as varchar) as ref_type, cast('sched_wakeup' as varchar) as name, cast(500000 as bigint) as ts, cast(2 as bigint) as wakeup_from
        union all
        select cast(1 as bigint) as ref, cast('itid' as varchar) as ref_type, cast('sched_wakeup' as varchar) as name, cast(800000 as bigint) as ts, cast(3 as bigint) as wakeup_from
        union all
        select cast(1 as bigint) as ref, cast('itid' as varchar) as ref_type, cast('sched_wakeup' as varchar) as name, cast(1050000 as bigint) as ts, cast(4 as bigint) as wakeup_from
        """
    ).write_parquet(str(tables_path / "instant.parquet"))
    ctx.sql(
        """
        select cast(1 as bigint) as itid, cast(10 as bigint) as tid, cast('root' as varchar) as name, cast(100 as bigint) as ipid
        union all
        select cast(2 as bigint) as itid, cast(20 as bigint) as tid, cast('udk-irq' as varchar) as name, cast(200 as bigint) as ipid
        union all
        select cast(3 as bigint) as itid, cast(30 as bigint) as tid, cast('short-waker' as varchar) as name, cast(300 as bigint) as ipid
        union all
        select cast(4 as bigint) as itid, cast(40 as bigint) as tid, cast('late-waker' as varchar) as name, cast(400 as bigint) as ipid
        """
    ).write_parquet(str(tables_path / "thread.parquet"))
    ctx.sql(
        """
        select cast(100 as bigint) as ipid, cast(1000 as bigint) as pid, cast('root-process' as varchar) as name
        union all
        select cast(200 as bigint) as ipid, cast(2000 as bigint) as pid, cast('irq-process' as varchar) as name
        union all
        select cast(300 as bigint) as ipid, cast(3000 as bigint) as pid, cast('short-process' as varchar) as name
        union all
        select cast(400 as bigint) as ipid, cast(4000 as bigint) as pid, cast('late-process' as varchar) as name
        """
    ).write_parquet(str(tables_path / "process.parquet"))
    ctx.sql(
        """
        select cast(1 as bigint) as callid, cast(0 as bigint) as ts, cast(1000000 as bigint) as dur, cast('root-stack' as varchar) as name
        union all
        select cast(2 as bigint) as callid, cast(100000 as bigint) as ts, cast(300000 as bigint) as dur, cast('irq-stack' as varchar) as name
        """
    ).write_parquet(str(tables_path / "callstack.parquet"))

    (dataset_path / "catalog.json").write_text(
        json.dumps(
            {
                "tables": [
                    {"name": "thread_state", "path": "tables/thread_state.parquet", "kind": "source"},
                    {"name": "instant", "path": "tables/instant.parquet", "kind": "source"},
                    {"name": "thread", "path": "tables/thread.parquet", "kind": "source"},
                    {"name": "process", "path": "tables/process.parquet", "kind": "source"},
                    {"name": "callstack", "path": "tables/callstack.parquet", "kind": "source"},
                ]
            }
        ),
        encoding="utf-8",
    )

    pack_root = tmp_path / "pack"
    pack_root.mkdir()
    critical_path_file = REPO_ROOT / "packs" / "openharmony-critical-path" / "compute" / "critical_path.py"
    (pack_root / "pack.py").write_text(
        f"""
from importlib.util import module_from_spec, spec_from_file_location
from kat import workflow

_spec = spec_from_file_location("critical_path_module", {str(critical_path_file)!r})
_module = module_from_spec(_spec)
_spec.loader.exec_module(_module)

@workflow(title="Critical path fixture", description="Run critical path fixture")
def critical_path_fixture(kat):
    return _module.critical_path(
        kat,
        root_itid=1,
        start_ts=0,
        end_ts=1000000,
        max_depth=1,
        min_segment_ms=0.2,
    )
""",
        encoding="utf-8",
    )

    run_dir = tmp_path / "run"
    request = {
        "packRoot": str(pack_root),
        "workflow": "critical_path_fixture",
        "datasetPath": str(dataset_path),
        "runDir": str(run_dir),
        "inputs": {},
    }
    request_path = tmp_path / "request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    subprocess.run(
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

    nodes = _read_parquet(run_dir / "artifacts" / "path_nodes.parquet")
    edges = _read_parquet(run_dir / "artifacts" / "path_edges.parquet")

    assert set(nodes["itid"]) == {1, 2}
    root_index = nodes["itid"].index(1)
    assert nodes["segment_start_ts"][root_index] == 0
    assert nodes["segment_end_ts"][root_index] == 1000000
    assert nodes["dur"][root_index] == 1000000

    irq_index = nodes["itid"].index(2)
    assert nodes["thread_name"][irq_index] == "udk-irq"
    assert nodes["segment_start_ts"][irq_index] == 100000
    assert nodes["segment_end_ts"][irq_index] == 400000
    assert nodes["dur"][irq_index] == 300000

    assert edges["from_itid"] == [2]
    assert edges["to_itid"] == [1]
    assert edges["wakeup_ts"] == [500000]


def _read_parquet(path: Path) -> dict[str, list]:
    import datafusion

    ctx = datafusion.SessionContext()
    batches = ctx.read_parquet(str(path)).collect()
    rows: dict[str, list] = {}
    for batch in batches:
        for key, values in batch.to_pydict().items():
            rows.setdefault(key, []).extend(values)
    return rows
