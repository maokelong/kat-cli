#!/usr/bin/env python3
"""验证 pip 安装、原环境升级及升级前后的实际 Hitrace 执行链路。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

from create_payload_smoke_hitrace import create_fixture


REPOSITORY = Path(__file__).resolve().parents[2]


def run(*arguments: object, **kwargs: object) -> subprocess.CompletedProcess:
    try:
        return subprocess.run([str(value) for value in arguments], check=True, **kwargs)
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", file=sys.stderr)
        print(error.stderr or "", file=sys.stderr)
        raise


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_previous_wheels(wheels: Path, work: Path) -> tuple[Path, str, str]:
    sdk, = wheels.glob("kat_sdk-*.whl")
    version = sdk.name.split("-")[1]
    previous = version + ".dev0"
    old = work / "previous"
    old.mkdir()
    for wheel in wheels.glob("*.whl"):
        run(sys.executable, "-m", "wheel", "unpack", wheel, "--dest", work / "unpacked")
        root = work / "unpacked" / wheel.name.split("-cp")[0].split("-py3")[0]
        info, = root.glob("*.dist-info")
        metadata = info / "METADATA"
        metadata.write_text(metadata.read_text(encoding="utf-8")
                            .replace(f"Version: {version}", f"Version: {previous}")
                            .replace(f"=={version}", f"=={previous}")
                            .replace(f">={version}", f">={previous}"), encoding="utf-8")
        info.rename(info.with_name(info.name.replace(version, previous)))
        if wheel.name.startswith("kat_sdk-"):
            (root / "kat/_runtime/obsolete-upgrade-fixture.txt").write_text("old version only", encoding="utf-8")
        run(sys.executable, "-m", "wheel", "pack", root, "--dest-dir", old)
    return old, version, previous


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheels", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    wheels = args.wheels.resolve()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=False)
    for wheel in wheels.glob("*.whl"):
        with zipfile.ZipFile(wheel) as archive:
            assert not any(Path(name).name in {"python", "python.exe", "python3"} for name in archive.namelist())
    old, version, previous = test_previous_wheels(wheels, work)
    skill = work / "kat"
    target = "windows-x86_64" if os.name == "nt" else "linux-x86_64"
    payload = skill / "scripts" / "targets" / target
    payload.mkdir(parents=True)
    shutil.copy2(REPOSITORY / "kat/skills/kat/SKILL.md", skill / "SKILL.md")
    shutil.copytree(REPOSITORY / "kat/packs", skill / "assets/packs")
    cli = payload / ("kat.exe" if os.name == "nt" else "kat")
    shutil.copy2(args.cli.resolve(strict=True), cli)
    # 测试用 venv 按现有 CLI 测试方式放置相邻 Host；正式 Skill 仍使用可重定位 Python。
    environment_root = payload if os.name == "nt" else payload / "python"
    run(sys.executable, "-m", "venv", environment_root)
    if os.name == "nt":
        python = payload / "python/python.exe"
        python.parent.mkdir()
        shutil.copy2(environment_root / "Scripts/python.exe", python)
    else:
        python = environment_root / "bin/python3"
    run(python, "-m", "pip", "install", "--only-binary=:all:", "--find-links", old, f"kat-sdk=={previous}")
    run(python, "-m", "pip", "check")
    run(python, "-I", "-B", REPOSITORY / "build/fixtures/verify_installed_sdk.py")
    site = Path(run(python, "-c", "import sysconfig; print(sysconfig.get_path('platlib'))", capture_output=True, text=True).stdout.strip())
    unchanged = [python, environment_root / "pyvenv.cfg", site / "pyarrow/__init__.py", site / "datafusion/__init__.py"]
    unchanged.extend((site / "pyarrow").glob("*.pyd" if os.name == "nt" else "*.so"))
    before = {str(path): (digest(path), path.stat().st_mtime_ns) for path in unchanged}
    pack = work / "user-pack"
    shutil.copytree(REPOSITORY / "build/fixtures/payload-smoke-pack", pack)
    # 真实 Workflow 断言 Runtime 来自本次安装环境，防止误用系统或相邻 Python。
    workflow = pack / "workflows/summarize_hitrace_clock.py"
    code = workflow.read_text(encoding="utf-8")
    code = code.replace("    provider = HitraceProvider(",
                        "    import sys\n"
                        f"    assert sys.executable == {str(python)!r}\n"
                        "    provider = HitraceProvider(")
    workflow.write_text(code, encoding="utf-8")
    pack_before = {str(path.relative_to(pack)): digest(path) for path in pack.rglob("*") if path.is_file()}
    data_home = work / "user-data"
    data_home.mkdir()
    sentinel = data_home / "keep.txt"
    sentinel.write_text("user data must survive", encoding="utf-8")
    trace = work / "trace.htrace"
    create_fixture(trace)
    env = dict(os.environ, KAT_DATA_HOME=str(data_home))
    # 使用私有 config 根，测试不依赖执行机器的用户配置。
    env.update(APPDATA=str(work / "config"), XDG_DATA_HOME=str(work / "config"))

    def command(*arguments: object) -> dict:
        result = run(cli, *arguments, env=env, capture_output=True, text=True, encoding="utf-8")
        response = json.loads(result.stdout)
        assert response["status"] == "success", response
        return response["result"]

    def query(session: str, run_id: str) -> None:
        result = command("query", "--session", session, "--run", run_id,
                         "--sql", "SELECT clock_domain, clock_value FROM output.main")
        assert Path(result["path"]).read_bytes() == b'{"clock_domain":"boottime","clock_value":123456}\n'

    def full_loop() -> tuple[str, str]:
        command("inspect", "--pack-dir", pack)
        command("inspect", "provider")
        command("inspect", "workflow", "--pack", "payload-smoke", "--pack-dir", pack)
        assert command("test", "--pack-dir", pack)["summary"] == {"passed": 1}
        session = command("session", "create")["session_id"]
        result = command("run", "--session", session, "--pack", "payload-smoke", "--workflow",
                         "summarize-hitrace-clock", "--pack-dir", pack, "--", "--trace-path", trace)
        assert result["outputs"]["main"]["row_count"] == 1
        query(session, result["run_id"])
        return session, result["run_id"]

    assert (site / "kat/_runtime/obsolete-upgrade-fixture.txt").is_file()
    cli_before = (digest(cli), cli.stat().st_mtime_ns)
    skill_before = digest(skill / "SKILL.md")
    session, run_id = full_loop()
    run(python, "-m", "pip", "install", "--upgrade", "--no-index", "--only-binary=:all:",
        "--find-links", wheels, f"kat-sdk=={version}")
    run(python, "-m", "pip", "check")
    assert before == {str(path): (digest(path), path.stat().st_mtime_ns) for path in unchanged}
    assert not (site / "kat/_runtime/obsolete-upgrade-fixture.txt").exists()
    run(python, "-I", "-B", REPOSITORY / "build/fixtures/verify_installed_sdk.py")
    assert sentinel.read_text(encoding="utf-8") == "user data must survive"
    assert pack_before == {str(path.relative_to(pack)): digest(path) for path in pack.rglob("*") if path.is_file()}
    assert cli_before == (digest(cli), cli.stat().st_mtime_ns)
    assert skill_before == digest(skill / "SKILL.md")
    query(session, run_id)
    full_loop()
    for example in ("local-parquet-fusion", "mem-pack", "workflow-composition"):
        result = command("test", "--pack-dir", REPOSITORY / "examples/packs" / example)
        assert result["summary"].get("passed", 0) > 0, result
    print(json.dumps({"sdk_version": version, "python": str(python),
                      "same_python_cli_and_unchanged_dependencies": True,
                      "external_pack_and_data_preserved": True,
                      "inspect_test_run_query_before_and_after_upgrade": True}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
