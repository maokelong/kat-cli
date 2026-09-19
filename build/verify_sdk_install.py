#!/usr/bin/env python3
"""在新建的隔离 KAT 部署中验证真实 SDK wheel，以及仅 SDK 的 pip 升级。"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

from build_sdk_wheel import build_sdk_wheel, validate_sdk_wheel_archive


def run(*arguments: object, **kwargs) -> str:
    completed = subprocess.run([str(x) for x in arguments], capture_output=True,
                               text=True, encoding="utf-8", **kwargs)
    if completed.returncode:
        raise RuntimeError(f"Command failed: {arguments}\n{completed.stdout}\n{completed.stderr}")
    return completed.stdout


def fixture(repository: Path, work: Path, revision: int) -> Path:
    source = work / f"source-{revision}"
    sdk = source / "kat/sdk"
    shutil.copytree(repository / "kat/sdk", sdk,
                    ignore=shutil.ignore_patterns("build", "*.egg-info", "__pycache__"))
    config = sdk / "pyproject.toml"
    original = config.read_text(encoding="utf-8")
    version = tomllib.loads(original)["project"]["version"]
    config.write_text(original.replace(f'version = "{version}"', f'version = "{version}+verify{revision}"')
                      .replace('"kat_sdk.providers"]', '"kat_sdk.providers", "kat_sdk.libraries"]'), encoding="utf-8")
    def write(path: str, text: str) -> None:
        target = sdk / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    write("libraries/__init__.py", "")
    write("libraries/values.py", f'''__all__ = ["increment"]

def increment(value: int) -> int:
    """增加输入整数，用于验证中文、类型注解与实际调用。

    Examples:
        >>> increment(40)
        {40 + revision}
    """
    return value + {revision}
''')
    workflow = '''import kat
import pyarrow as pa
from kat import dataprovider as dp
from kat_sdk.libraries.values import increment

@kat.workflow(name="sdk-probe", description="SDK verification workflow.", parameters={"value": "Input value"}, guide="workflows/probe.md")
def probe(ctx: kat.Context, value: int = 40):
    """SDK verification workflow."""
    return dp.Table.from_arrow(pa.table({"value": [increment(value)]}))
'''
    write("workflows/verification/probe.py", workflow)
    write("knowledge/workflows/probe.md", f"# SDK probe revision {revision}\n")
    index = sdk / "knowledge/index.md"
    index.write_text(index.read_text(encoding="utf-8") + "\n- [Verification function](libraries/values.api.md)\n- [Verification Workflow](workflows/probe.md)\n", encoding="utf-8")
    if revision == 1:
        write("libraries/removed.py", '"""此旧版本文件必须由 pip 在升级时移除。"""\n')
    else:
        write("providers/probe.py", '''import kat
__all__ = ["ProbeProvider"]

@kat.provider(name="sdk-probe", description="Upgrade verification provider.", guide="providers/probe.md")
class ProbeProvider:
    """仅用于安装升级验证。"""
    def value(self) -> int:
        """返回升级后的实现值。"""
        return 42
''')
        with (sdk / "__init__.py").open("a", encoding="utf-8") as stream:
            stream.write('\nPROVIDER_MODULES += ("kat_sdk.providers.probe",)\n')
        write("knowledge/providers/probe.md", "# SDK provider revision 2\n")
        write("workflows/verification/added.py", workflow.replace('name="sdk-probe"', 'name="sdk-added"'))
    wheel, _ = build_sdk_wheel(source, work / f"fixture-wheel-{revision}")
    return wheel


def verify(options: argparse.Namespace) -> dict:
    repository = Path(__file__).resolve().parents[1]
    version = validate_sdk_wheel_archive(options.sdk_wheel)
    root = options.output.resolve()
    if root.exists():
        raise ValueError(f"Verification output already exists: {root}")
    root.mkdir(parents=True)
    skill = root / "skill"
    skill.mkdir()
    (skill / "SKILL.md").write_text("# SDK verification\n", encoding="utf-8")
    payload = skill / "scripts/targets" / ("windows-x86_64" if os.name == "nt" else "linux-x86_64")
    environment = payload if os.name == "nt" else payload / "python"
    run(sys.executable, "-m", "venv", environment)
    python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python3")
    run(python, "-m", "pip", "install", "--disable-pip-version-check",
        "--find-links", options.workflow_wheel.parent,
        options.workflow_wheel, options.datasource_wheel, options.sdk_wheel)
    host = payload / ("python/python.exe" if os.name == "nt" else "python/bin/python3")
    if os.name == "nt":
        host.parent.mkdir()
        shutil.copy2(python, host)
    cli = payload / ("kat.exe" if os.name == "nt" else "kat")
    shutil.copy2(options.kat, cli)
    home = root / "data"
    home.mkdir()
    env = {**os.environ, "KAT_DATA_HOME": str(home), "PYTHONUTF8": "1"}
    def invoke(*args: object, success: bool = True) -> dict:
        result = subprocess.run([str(cli), *map(str, args)], cwd=root, env=env,
                                capture_output=True, text=True, encoding="utf-8")
        response = json.loads(result.stdout)
        assert (result.returncode == 0) == success, (response, result.stderr)
        assert response["status"] == ("success" if success else "failure"), response
        return response
    def host_run(script: str) -> str:
        return run(host, "-I", "-B", "-X", "utf8", "-c", script)
    identity_script = "import importlib.metadata as m; print(m.version('kat-workflow')); print(m.version('kat-datasource'))"
    host_run("import importlib.util as u; assert u.find_spec('kat.dataprovider.ftrace') is None; assert u.find_spec('kat.dataprovider.trace_streamer') is None")
    before = host_run(identity_script)
    binary_hash = hashlib.sha256(cli.read_bytes()).hexdigest()
    run(host, "-m", "pip", "check")
    packs = invoke("inspect")["result"]["packs"]
    assert "kat-sdk" in [p["name"] for p in packs], packs
    assert invoke("inspect", "workflow", "--pack", "kat-sdk")["result"]["workflows"] == []
    public = invoke("inspect", "provider")["result"]["providers"]
    assert [p["name"] for p in public] == ["ftrace-text", "trace-streamer-sqlite"]
    for name in ("ftrace-text", "trace-streamer-sqlite"):
        detail = invoke("inspect", "provider", "--provider", name)["result"]["provider"]
        assert detail["module"].startswith("kat_sdk.providers.") and detail["guide"].strip(), detail
    # SDK tests run against the installed wheel; the repository source is never added to sys.path.
    run(host, "-I", "-B", "-X", "utf8", "-m", "pytest", "-q", "-p", "no:cacheprovider", repository / "kat/sdk/tests")
    library = host_run("from importlib.resources import files; print(files('kat_sdk'))").strip()
    sdk_root = Path(library)
    assert (sdk_root / "knowledge/providers/ftrace.api.md").is_file()
    assert invoke("inspect", "--pack-dir", sdk_root)["result"]["packs"] == packs
    manifest = sdk_root / "pack.toml"
    manifest_bytes = manifest.read_bytes()
    manifest.unlink()
    try:
        invoke("inspect", success=False)
        invoke("inspect", "provider")
    finally:
        manifest.write_bytes(manifest_bytes)
    duplicate = root / "duplicate"
    duplicate.mkdir()
    shutil.copy2(sdk_root / "pack.toml", duplicate / "pack.toml")
    invoke("inspect", "--pack-dir", duplicate, success=False)
    # An unrelated broken PACK must not block public Provider discovery.
    broken = home / "packs/broken"
    broken.mkdir(parents=True)
    (broken / "pack.toml").write_text("invalid TOML", encoding="utf-8")
    invoke("inspect", success=False)
    invoke("inspect", "provider")
    (broken / "pack.toml").unlink()
    guide = sdk_root / "knowledge/providers/ftrace.md"
    original = guide.read_bytes()
    guide.unlink()
    try:
        invoke("inspect", "provider", success=False)
    finally:
        guide.write_bytes(original)
    consumer = root / "consumer"
    (consumer / "workflows").mkdir(parents=True)
    (consumer / "tests").mkdir()
    (consumer / "pack.toml").write_text('name="consumer"\ntitle="Consumer"\ndescription="SDK caller"\nowner="KAT tests"\n', encoding="utf-8")
    (consumer / "workflows/call.py").write_text('''import kat
@kat.workflow(name="call-sdk", description="Call the installed SDK.")
def call(ctx: kat.Context):
    """Call the installed SDK."""
    catalog = ctx.run("kat-sdk", "sdk-probe")
    return kat.dataprovider.DataFusionProvider(catalog=catalog).query("SELECT value FROM main")
''', encoding="utf-8")
    revisions = []
    for revision in (1, 2):
        wheel = fixture(repository, root, revision)
        run(host, "-m", "pip", "install", "--no-deps", "--no-index", "--upgrade", wheel)
        listing = invoke("inspect", "workflow", "--pack", "kat-sdk")["result"]["workflows"]
        expected = ["sdk-probe"] if revision == 1 else ["sdk-added", "sdk-probe"]
        assert [w["name"] for w in listing] == expected, listing
        detail = invoke("inspect", "workflow", "--pack", "kat-sdk", "--workflow", "sdk-probe")["result"]["workflow"]
        assert f"revision {revision}" in detail["guide"] and detail["parameters"], detail
        session = invoke("session", "create")["result"]["session_id"]
        for pack, workflow, extra in (("kat-sdk", "sdk-probe", []), ("consumer", "call-sdk", ["--pack-dir", consumer])):
            executed = invoke("run", "--session", session, "--pack", pack, "--workflow", workflow, *extra)
            assert executed["result"]["outputs"]["main"]["row_count"] == 1, executed
            published = home / "sessions" / session / "runs" / executed["result"]["run_id"] / "outputs/main.parquet"
            assert json.loads(host_run(f"import json, pyarrow.parquet as p; print(json.dumps(p.read_table({str(published)!r}).to_pydict()))")) == {"value": [40 + revision]}
        (consumer / "tests/test_sdk.py").write_text(f'''def test_sdk(kat_run):
    assert kat_run(workflow="call-sdk")["main"].to_pydict() == {{"value": [{40 + revision}]}}
''', encoding="utf-8")
        tested = invoke("test", "--pack-dir", consumer)
        assert tested["result"]["summary"]["passed"] == 1, tested
        knowledge = (sdk_root / "knowledge/libraries/values.api.md").read_text(encoding="utf-8")
        assert "increment(value: int) -> int" in knowledge and "增加输入整数" in knowledge, knowledge
        assert "libraries/values.api.md" in (sdk_root / "knowledge/index.md").read_text(encoding="utf-8")
        assert host_run("from kat_sdk.libraries.values import increment; print(increment(40))").strip() == str(40 + revision)
        if revision == 2:
            assert not (sdk_root / "libraries/removed.py").exists()
            assert not (sdk_root / "knowledge/libraries/removed.api.md").exists()
            detail = invoke("inspect", "provider", "--provider", "sdk-probe")["result"]["provider"]
            assert "revision 2" in detail["guide"]
            assert host_run("from kat_sdk.providers.probe import ProbeProvider; print(ProbeProvider().value())").strip() == "42"
        invoke("inspect", "provider", "--provider", "ftrace-text")
        assert host_run(identity_script) == before
        assert hashlib.sha256(cli.read_bytes()).hexdigest() == binary_hash
        revisions.append(wheel.name)
    run(host, "-I", "-B", "-X", "utf8", "-m", "pytest", "-q", "-p", "no:cacheprovider", repository / "kat/sdk/tests")
    run(host, "-m", "pip", "check")
    report = {"platform": sys.platform, "python": sys.version, "sdk_version": version,
              "sdk_sha256": hashlib.sha256(options.sdk_wheel.read_bytes()).hexdigest(),
              "framework_versions": before.splitlines(), "cli_sha256": binary_hash,
              "upgrade_wheels": revisions, "status": "passed"}
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("kat", "workflow-wheel", "datasource-wheel", "sdk-wheel", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    options = parser.parse_args()
    for name in ("kat", "workflow_wheel", "datasource_wheel", "sdk_wheel"):
        setattr(options, name, getattr(options, name).resolve(strict=True))
    print(json.dumps(verify(options), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
