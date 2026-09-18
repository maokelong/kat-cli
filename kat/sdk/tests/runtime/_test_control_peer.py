from __future__ import annotations

from contextlib import suppress
import json
from functools import cache
import os
import shutil
import site
import sys
import tempfile
from pathlib import Path
import subprocess
import threading
from typing import BinaryIO
import uuid


_PROCESS_TIMEOUT_SECONDS = 60.0
_SHUTDOWN_TIMEOUT_SECONDS = 5.0


def run_runtime_with_test_control(
    arguments: list[str],
    *,
    cwd: Path,
    environment: dict[str, str],
    data_home: Path,
    process_timeout_seconds: float = _PROCESS_TIMEOUT_SECONDS,
) -> subprocess.CompletedProcess[bytes]:
    process = subprocess.Popen(
        arguments,
        cwd=cwd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    )
    peer_errors: list[BaseException] = []
    stderr_errors: list[BaseException] = []
    stderr_chunks: list[bytes] = []
    peer = threading.Thread(
        target=_serve_test_control,
        args=(process, data_home, peer_errors, cwd, environment),
        name="fake-kat-test-host",
        daemon=True,
    )
    assert process.stderr is not None
    stderr_reader = threading.Thread(
        target=_drain_stderr,
        args=(process.stderr, stderr_chunks, stderr_errors),
        name="fake-kat-test-host-stderr",
        daemon=True,
    )
    peer.start()
    stderr_reader.start()

    timed_out: subprocess.TimeoutExpired | None = None
    reap_error: subprocess.TimeoutExpired | None = None
    try:
        try:
            returncode = process.wait(timeout=process_timeout_seconds)
        except subprocess.TimeoutExpired as error:
            timed_out = error
            process.kill()
            try:
                returncode = process.wait(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired as error:
                reap_error = error
                returncode = process.returncode if process.returncode is not None else -1

        peer.join(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
        stderr_reader.join(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
        stderr = b"".join(stderr_chunks)

        if reap_error is not None:
            raise RuntimeError(
                "Workflow Runtime test process could not be reaped within "
                f"{_SHUTDOWN_TIMEOUT_SECONDS:g} seconds after it was killed"
            ) from reap_error
        if peer.is_alive() or stderr_reader.is_alive():
            stalled = []
            if peer.is_alive():
                stalled.append("test-control peer")
            if stderr_reader.is_alive():
                stalled.append("stderr reader")
            raise RuntimeError(
                f"Workflow Runtime test process left a stalled {' and '.join(stalled)}"
            )
        if timed_out is not None:
            diagnostic = stderr.decode(errors="replace")
            suffix = f"\nstderr:\n{diagnostic}" if diagnostic else ""
            raise RuntimeError(
                "Workflow Runtime test process did not exit within "
                f"{process_timeout_seconds:g} seconds and was killed{suffix}"
            ) from timed_out
        if peer_errors:
            raise peer_errors[0]
        if stderr_errors:
            raise stderr_errors[0]
        return subprocess.CompletedProcess(
            arguments,
            returncode,
            stdout=b"",
            stderr=stderr,
        )
    finally:
        if process.poll() is None:
            process.kill()
            try:
                process.wait(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                pass
        peer.join(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
        stderr_reader.join(timeout=_SHUTDOWN_TIMEOUT_SECONDS)
        if process.stdin is not None and not peer.is_alive():
            with suppress(OSError):
                process.stdin.close()
        if process.stdout is not None and not peer.is_alive():
            with suppress(OSError):
                process.stdout.close()
        if process.stderr is not None and not stderr_reader.is_alive():
            with suppress(OSError):
                process.stderr.close()


def _drain_stderr(
    stderr: BinaryIO,
    chunks: list[bytes],
    errors: list[BaseException],
) -> None:
    try:
        for chunk in iter(lambda: stderr.read(64 * 1024), b""):
            chunks.append(chunk)
    except BaseException as error:
        errors.append(error)


@cache
def _real_host() -> tuple[tempfile.TemporaryDirectory, Path]:
    # 仅布置真实 CLI/已安装 Python 依赖；不在测试替身中重建 Run 执行与发布。
    repository = Path(__file__).resolve().parents[4]
    binary = Path(os.environ.get("KAT_TEST_KAT", repository / "target" / "debug" / ("kat.exe" if os.name == "nt" else "kat")))
    if not binary.is_file():
        raise RuntimeError("kat_run integration tests require cargo build -p kat-cli or KAT_TEST_KAT")
    temporary = tempfile.TemporaryDirectory(prefix="kat-python-tests-host-")
    root = Path(temporary.name)
    (root / "SKILL.md").write_text("# KAT test Host\n", encoding="utf-8")
    payload = root / "scripts" / "targets" / ("windows-x86_64" if os.name == "nt" else "linux-x86_64")
    environment = payload if os.name == "nt" else payload / "python"
    subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(environment)], check=True, capture_output=True)
    if os.name == "nt":
        libraries = environment / "Lib" / "site-packages"
        host = payload / "python" / "python.exe"
        host.parent.mkdir()
        shutil.copy2(environment / "Scripts" / "python.exe", host)
    else:
        libraries = environment / "lib" / f"python{sys.version_info.major}.{sys.version_info.minor}" / "site-packages"
    (libraries / "installed-test-dependencies.pth").write_text("\n".join(site.getsitepackages()) + "\n", encoding="utf-8")
    staged = payload / binary.name
    shutil.copy2(binary, staged)
    return temporary, staged


def _serve_test_control(
    process: subprocess.Popen[bytes],
    data_home: Path,
    errors: list[BaseException],
    pack: Path,
    environment: dict[str, str],
) -> None:
    sessions: dict[str, str] = {}
    data_home.mkdir(parents=True, exist_ok=True)

    def cli(*args: str) -> dict:
        _, binary = _real_host()
        completed = subprocess.run(
            [str(binary), *args], cwd=pack,
            env={**environment, "KAT_DATA_HOME": str(data_home)},
            capture_output=True, timeout=60,
        )
        return json.loads(completed.stdout)

    try:
        assert process.stdout is not None
        assert process.stdin is not None
        for raw_frame in iter(process.stdout.readline, b""):
            frame = json.loads(raw_frame)
            call_id = frame["call_id"]
            operation = frame["operation"]
            if operation == "begin_test_session":
                result = cli("session", "create")
                if result["status"] != "success":
                    raise AssertionError(result)
                capability = uuid.uuid4().hex
                sessions[capability] = result["result"]["session_id"]
                response = {"call_id": call_id, "status": "success", "test_session_id": capability}
            elif operation == "end_test_session":
                del sessions[frame["test_session_id"]]
                response = {"call_id": call_id, "status": "success"}
            elif operation == "run_workflow":
                session_id = sessions[frame["test_session_id"]]
                manifest = pack / "pack.toml"
                if not manifest.exists():
                    manifest.write_text(
                        f'name = {json.dumps(frame["pack_name"])}\ntitle = "Test PACK"\ndescription = "Real Host fixture"\nowner = "KAT tests"\n',
                        encoding="utf-8",
                    )
                result = cli("run", "--session", session_id, "--pack", frame["pack_name"],
                    "--workflow", frame["workflow_name"], "--pack-dir", str(pack), "--", *frame["arguments"])
                if result["status"] == "failure":
                    diagnostic = result["error"]
                    message = ": ".join([diagnostic["message"], *diagnostic.get("causes", [])])
                    response = {"call_id": call_id, "status": "failure", "message": message}
                else:
                    run = result["result"]
                    outputs = data_home / "sessions" / session_id / "runs" / run["run_id"] / "outputs"
                    response = {"call_id": call_id, "status": "success", "relations": [
                        {"name": name, "path": str((outputs / f"{name}.parquet").resolve())}
                        for name in sorted(run["outputs"])
                    ]}
            else:
                raise AssertionError(f"unexpected test control operation: {operation}")
            process.stdin.write(json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\n")
            process.stdin.flush()
    except BaseException as error:
        errors.append(error)
        process.kill()
