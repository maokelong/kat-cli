from __future__ import annotations

import argparse
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import venv
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
SMOKE = REPO_ROOT / "python" / "tests" / "wheel_smoke.py"


def run(command: list[str], *, env=None, cwd=None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, env=env, cwd=cwd, check=True)


def venv_python(environment: Path) -> Path:
    if os.name == "nt":
        return environment / "Scripts" / "python.exe"
    return environment / "bin" / "python"


def create_venv(path: Path) -> Path:
    if path.exists():
        raise FileExistsError(f"clean venv target already exists: {path}")
    venv.EnvBuilder(with_pip=True, clear=False).create(path)
    return venv_python(path)


def clean_env() -> dict[str, str]:
    environment = os.environ.copy()
    environment.pop("PYTHONPATH", None)
    environment.pop("PYTHONHOME", None)
    return environment


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--runtime-venv",
        type=Path,
        help="retain the clean Runtime venv at this new path for CLI E2E",
    )
    args = parser.parse_args()
    if importlib.util.find_spec("build") is None:
        raise RuntimeError("install the build package with: python -m pip install build")

    with tempfile.TemporaryDirectory(
        prefix=".kat-wheel-verify-",
        dir=REPO_ROOT,
    ) as temporary:
        work = Path(temporary)
        dist = work / "dist"
        dist.mkdir()
        source_ignore = shutil.ignore_patterns(
            "build",
            "*.egg-info",
            "__pycache__",
            "*.pyc",
        )
        sdk_build_root = work / "kat-python-sdk"
        runtime_build_root = work / "kat-python-runtime"
        shutil.copytree(SDK_ROOT, sdk_build_root, ignore=source_ignore)
        shutil.copytree(RUNTIME_ROOT, runtime_build_root, ignore=source_ignore)
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--outdir",
                str(dist),
                str(sdk_build_root),
            ]
        )
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--outdir",
                str(dist),
                str(runtime_build_root),
            ]
        )
        sdk_wheel = next(dist.glob("kat_python_sdk-0.1.0-*.whl"))
        runtime_wheel = next(dist.glob("kat_python_runtime-0.1.0-*.whl"))

        sdk_python = create_venv(work / "sdk-only-venv")
        run(
            [
                str(sdk_python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--no-deps",
                str(sdk_wheel),
            ],
            env=clean_env(),
        )
        sdk_probe = """
import importlib.util
import pathlib
import sysconfig
import kat

purelib = pathlib.Path(sysconfig.get_paths()["purelib"]).resolve()
module_path = pathlib.Path(kat.__file__).resolve()
assert module_path.is_relative_to(purelib), (module_path, purelib)
assert (module_path.parent / "py.typed").is_file()
assert importlib.util.find_spec("datafusion") is None
"""
        run([str(sdk_python), "-I", "-c", sdk_probe], env=clean_env())

        runtime_venv = (
            args.runtime_venv.resolve()
            if args.runtime_venv is not None
            else work / "runtime-venv"
        )
        runtime_python = create_venv(runtime_venv)
        run(
            [
                str(runtime_python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--only-binary=:all:",
                str(sdk_wheel),
                str(runtime_wheel),
            ],
            env=clean_env(),
        )
        run([str(runtime_python), "-m", "pip", "check"], env=clean_env())

        smoke_root = work / "smoke"
        smoke_root.mkdir()
        smoke_env = clean_env()
        smoke_env["KAT_WHEEL_SMOKE_ROOT"] = str(smoke_root)
        run(
            [str(runtime_python), "-I", str(SMOKE)],
            env=smoke_env,
            cwd=work,
        )
        print(f"clean Runtime Python: {runtime_python}")
        print("wheel verification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
