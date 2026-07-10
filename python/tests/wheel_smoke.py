from __future__ import annotations

import importlib.metadata
import json
import os
import subprocess
import sys
import sysconfig
from pathlib import Path

import datafusion
import kat
import kat_runtime
from datafusion import DataFrame, SessionContext
from kat import Kat


def assert_site_package(module) -> None:
    purelib = Path(sysconfig.get_paths()["purelib"]).resolve()
    module_path = Path(module.__file__).resolve()
    assert module_path.is_relative_to(purelib), (module.__name__, module_path, purelib)


def main() -> int:
    assert datafusion.__version__ == "54.0.0"
    assert importlib.metadata.version("kat-python-sdk") == "0.1.0"
    assert importlib.metadata.version("kat-python-runtime") == "0.1.0"
    runtime_requirements = set(
        importlib.metadata.requires("kat-python-runtime") or []
    )
    assert "kat-python-sdk==0.1.0" in runtime_requirements
    assert "datafusion==54.0.0" in runtime_requirements
    assert_site_package(kat)
    assert_site_package(kat_runtime)

    native = Kat(ctx=SessionContext()).sql(
        "select $value as value, $quoted as quoted",
        value=54,
        quoted="DataFusion's native parameter",
    )
    assert isinstance(native, DataFrame)
    assert native.to_pydict() == {
        "value": [54],
        "quoted": ["DataFusion's native parameter"],
    }

    root = Path(os.environ["KAT_WHEEL_SMOKE_ROOT"]).resolve()
    dataset = root / "dataset"
    tables = dataset / "tables"
    pack = root / "pack"
    run_dir = root / "run"
    tables.mkdir(parents=True)
    pack.mkdir(parents=True)

    SessionContext().sql(
        "select 405 as itid, 'main' as thread_name"
    ).write_parquet(tables / "thread.parquet")
    (dataset / "catalog.json").write_text(
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
    (pack / "pack.py").write_text(
        """
from kat import workflow

@workflow(title="Installed smoke", description="Use the installed runtime")
def installed_smoke(kat, min_itid: int):
    return {
        "path_nodes": kat.sql(
            "select itid, thread_name from thread where itid >= $min_itid",
            min_itid=min_itid,
        )
    }
""",
        encoding="utf-8",
    )

    child_env = os.environ.copy()
    child_env.pop("PYTHONPATH", None)
    child_env.pop("PYTHONHOME", None)
    discovery = subprocess.run(
        [
            sys.executable,
            "-I",
            "-m",
            "kat_runtime.worker.discovery",
            "--pack-root",
            str(pack),
        ],
        env=child_env,
        cwd=root,
        text=True,
        capture_output=True,
        check=True,
    )
    assert [item["name"] for item in json.loads(discovery.stdout)["workflows"]] == [
        "installed_smoke"
    ]

    request_path = root / "request.json"
    request_path.write_text(
        json.dumps(
            {
                "packRoot": str(pack),
                "workflow": "installed_smoke",
                "datasetPath": str(dataset),
                "runDir": str(run_dir),
                "inputs": {"min_itid": 400},
            }
        ),
        encoding="utf-8",
    )
    subprocess.run(
        [
            sys.executable,
            "-I",
            "-m",
            "kat_runtime.worker.run",
            "--request",
            str(request_path),
        ],
        env=child_env,
        cwd=root,
        text=True,
        capture_output=True,
        check=True,
    )

    manifest = json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))
    artifact = run_dir / manifest["artifacts"][0]["path"]
    reader = SessionContext()
    reader.register_parquet("path_nodes", str(artifact))
    assert manifest["status"] == "success"
    assert artifact.is_file()
    assert reader.table("path_nodes").to_pydict() == {
        "itid": [405],
        "thread_name": ["main"],
    }
    print("installed wheel smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
