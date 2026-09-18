#!/usr/bin/env python3
"""在最终归档的可写重定位副本中验证原生 pip 和真实 PACK 执行。"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


_PROBE = """
import importlib.util, json, pathlib, sys, sysconfig
import packaging, pip
six_spec = importlib.util.find_spec('six')
print(json.dumps({
    'python': str(pathlib.Path(sys.executable).resolve()),
    'site_packages': str(pathlib.Path(sysconfig.get_path('purelib')).resolve()),
    'pip_file': str(pathlib.Path(pip.__file__).resolve()),
    'pip_version': pip.__version__,
    'packaging_file': str(pathlib.Path(packaging.__file__).resolve()),
    'packaging_version': packaging.__version__,
    'six_file': str(pathlib.Path(six_spec.origin).resolve()) if six_spec else None,
}))
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    parser.add_argument("wheelhouse", type=Path)
    parser.add_argument("work", type=Path)
    args = parser.parse_args()
    original_cli = args.cli.resolve(strict=True)
    wheelhouse = args.wheelhouse.resolve(strict=True)
    work = args.work.resolve()
    if work.is_relative_to(original_cli.parent):
        raise ValueError("The smoke work directory must be outside the input payload")
    work.mkdir(parents=True, exist_ok=False)
    def copy_payload(destination: Path) -> Path:
        skill = destination / "kat"
        payload = skill / "scripts" / "targets" / original_cli.parent.name
        shutil.copytree(original_cli.parent, payload, symlinks=True)
        shutil.copy2(original_cli.parents[3] / "SKILL.md", skill / "SKILL.md")
        return payload / original_cli.name

    cli = copy_payload(work / "relocated deployment")
    python_parts = ("python", "python.exe") if os.name == "nt" else ("python", "bin", "python3")
    python = cli.parent.joinpath(*python_parts)
    original_python = original_cli.parent.joinpath(*python_parts)
    data_home = work / "data-home"
    data_home.mkdir()
    environment = {**os.environ, "KAT_DATA_HOME": str(data_home)}
    pip_environment = {
        key: value for key, value in environment.items()
        if not key.upper().startswith(("PYTHON", "PIP_"))
    }
    pip_environment["PIP_CONFIG_FILE"] = os.devnull
    fixtures = Path(__file__).resolve().parent
    lock = fixtures.parent / "requirements-package-smoke.lock.txt"
    pack = fixtures / "pip-smoke-pack"

    def execute(label: str, command: list[str], *, pip: bool = False, succeeds: bool = True) -> str:
        completed = subprocess.run(
            command, cwd=work, env=pip_environment if pip else environment,
            stdin=subprocess.DEVNULL, capture_output=True, timeout=180,
            encoding="utf-8", errors="replace",
        )
        evidence = {
            "command": command, "returncode": completed.returncode,
            "stdout": completed.stdout, "stderr": completed.stderr,
        }
        (work / f"{label}.json").write_text(
            json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8",
        )
        print(f"{label}: {json.dumps(evidence)}", flush=True)
        if (completed.returncode == 0) != succeeds:
            raise RuntimeError(f"{label}: unexpected exit code {completed.returncode}")
        return completed.stdout

    def probe(label: str, executable: Path) -> dict:
        state = json.loads(execute(label, [str(executable), "-I", "-B", "-c", _PROBE]))
        if Path(state["python"]) != executable.resolve():
            raise RuntimeError(f"{label}: wrong interpreter: {state}")
        site_packages = Path(state["site_packages"])
        if not site_packages.is_relative_to(executable.parent.parent if os.name != "nt" else executable.parent):
            raise RuntimeError(f"{label}: site-packages is outside the bundled Python: {state}")
        for key in ("pip_file", "packaging_file", "six_file"):
            if state[key] and not Path(state[key]).is_relative_to(site_packages):
                raise RuntimeError(f"{label}: {key} is outside bundled site-packages: {state}")
        return state

    def pip(label: str, *arguments: str) -> str:
        return execute(label, [
            str(python), "-I", "-B", "-m", "pip", "--disable-pip-version-check",
            "--no-input", *arguments,
        ], pip=True)

    def invoke(label: str, *arguments: str) -> dict:
        response = json.loads(execute(label, [str(cli), *arguments]))
        if response.get("status") != "success":
            raise RuntimeError(f"{label}: {response}")
        return response["result"]

    original = probe("original-environment", original_python)
    baseline = probe("relocated-environment", python)
    if baseline["six_file"] is not None or original["six_file"] is not None:
        raise RuntimeError("six must be absent before the installation smoke")
    if baseline["packaging_version"] == "25.0":
        raise RuntimeError("Select a packaging smoke version different from the shipped baseline")
    pip("pip-version", "--version")
    install = (
        "install", "--no-index", "--find-links", str(wheelhouse),
        "--require-hashes", "-r", str(lock),
    )
    pip("install-packages", *install)
    installed = probe("installed-environment", python)
    if not installed["six_file"] or installed["packaging_version"] != "25.0":
        raise RuntimeError(f"The installed environment did not change as requested: {installed}")
    pip("pip-check", "check")
    launcher = python.parent / "Scripts" / "wheel.exe" if os.name == "nt" else python.parent / "wheel"
    console_version = execute("installed-console-script", [str(launcher), "version"], pip=True)
    if console_version.strip() != "wheel 0.48.0":
        raise RuntimeError(f"The installed console script returned an unexpected version: {console_version}")
    inspected = invoke(
        "inspect-workflow", "inspect", "workflow", "--pack", "pip-smoke",
        "--workflow", "installed-packages", "--pack-dir", str(pack),
    )
    if inspected["workflow"]["name"] != "installed-packages":
        raise RuntimeError(f"Inspection returned the wrong Workflow: {inspected}")
    tested = invoke("test-pack", "test", "--pack-dir", str(pack))
    if tested["summary"] != {"passed": 1}:
        raise RuntimeError(f"PACK tests did not pass exactly once: {tested}")
    session_id = invoke("create-session", "session", "create")["session_id"]
    run = invoke(
        "run-workflow", "run", "--session", session_id, "--pack", "pip-smoke",
        "--workflow", "installed-packages", "--pack-dir", str(pack),
    )
    queried = invoke(
        "query-output", "query", "--session", session_id, "--run", run["run_id"],
        "--sql", "SELECT value, six_version, packaging_version, six_file, packaging_file, python_executable FROM output.main",
    )
    if queried["format"] != "ndjson":
        raise RuntimeError(f"Query did not return NDJSON: {queried}")
    rows = [json.loads(line) for line in Path(queried["path"]).read_text(encoding="utf-8").splitlines()]
    expected = {
        "value": "KAT library install", "six_version": "1.17.0", "packaging_version": "25.0",
        "six_file": installed["six_file"], "packaging_file": installed["packaging_file"],
        "python_executable": installed["python"],
    }
    if rows != [expected]:
        raise RuntimeError(f"Run Output did not use the installed libraries: {rows}")
    print(f"queried-library-evidence: {json.dumps(rows)}", flush=True)
    pip("uninstall-six", "uninstall", "--yes", "six")
    removed = probe("uninstalled-environment", python)
    if removed["six_file"] is not None:
        raise RuntimeError(f"six remained importable after uninstall: {removed}")
    execute("import-removed-six", [str(python), "-I", "-B", "-c", "import six"], succeeds=False)
    pip("reinstall-packages", *install)
    if probe("reinstalled-environment", python) != installed:
        raise RuntimeError("Reinstall did not restore the installed library environment")
    pip("final-pip-check", "check")
    if probe("clean-original-environment", original_python) != original:
        raise RuntimeError("The clean original deployment was modified by the smoke")
    cli = copy_payload(work / "fresh replacement")
    python = cli.parent.joinpath(*python_parts)
    replacement = probe("replacement-environment", python)
    if replacement["six_file"] is not None or replacement["packaging_version"] != original["packaging_version"]:
        raise RuntimeError(f"The clean replacement carried user changes: {replacement}")
    pip("replacement-reinstall", *install)
    invoke(
        "replacement-inspect", "inspect", "workflow", "--pack", "pip-smoke",
        "--workflow", "installed-packages", "--pack-dir", str(pack),
    )
    pip("replacement-pip-check", "check")
    print("Python package installation smoke passed", flush=True)


if __name__ == "__main__":
    main()
