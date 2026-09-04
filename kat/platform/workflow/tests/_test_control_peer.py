from __future__ import annotations

from contextlib import suppress
import json
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
        args=(process, data_home, peer_errors),
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


def _serve_test_control(
    process: subprocess.Popen[bytes],
    data_home: Path,
    errors: list[BaseException],
) -> None:
    sessions: dict[str, Path] = {}
    runs: dict[str, str] = {}
    try:
        assert process.stdout is not None
        assert process.stdin is not None
        for raw_frame in iter(process.stdout.readline, b""):
            frame = json.loads(raw_frame)
            call_id = frame["call_id"]
            operation = frame["operation"]
            if operation == "begin_test_session":
                session_id = str(uuid.uuid7())
                session = data_home / "sessions" / session_id
                for name in ("materializations", "scratch", "runs"):
                    (session / name).mkdir(parents=True)
                capability = uuid.uuid4().hex
                sessions[capability] = session.resolve(strict=True)
                response = {
                    "call_id": call_id,
                    "status": "success",
                    "test_session_id": capability,
                }
            elif operation == "begin_test_run":
                session = sessions[frame["test_session_id"]]
                candidate_id = str(uuid.uuid7())
                candidate = session / "runs" / candidate_id
                scratch = session / "scratch" / candidate_id
                candidate.mkdir()
                scratch.mkdir()
                capability = uuid.uuid4().hex
                runs[capability] = frame["test_session_id"]
                response = {
                    "call_id": call_id,
                    "status": "success",
                    "test_run_id": capability,
                    "candidate_id": candidate_id,
                    "candidate_path": str(candidate.resolve(strict=True)),
                    "datasource_root": str(
                        (session / "materializations").resolve(strict=True)
                    ),
                    "scratch_root": str(scratch.resolve(strict=True)),
                }
            elif operation == "end_test_run":
                del runs[frame["test_run_id"]]
                response = {"call_id": call_id, "status": "success"}
            elif operation == "end_test_session":
                capability = frame["test_session_id"]
                if capability in runs.values():
                    raise AssertionError("Session ended with an active test Run")
                del sessions[capability]
                response = {"call_id": call_id, "status": "success"}
            elif operation == "run_workflow":
                if frame["test_run_id"] not in runs:
                    raise AssertionError("nested Run used an inactive test scope")
                response = {
                    "call_id": call_id,
                    "status": "failure",
                    "message": "fake Host does not execute nested Workflows",
                }
            else:
                raise AssertionError(f"unexpected test control operation: {operation}")
            process.stdin.write(
                json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\n"
            )
            process.stdin.flush()
    except BaseException as error:
        errors.append(error)
        process.kill()
