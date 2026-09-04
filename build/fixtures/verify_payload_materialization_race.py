#!/usr/bin/env python3
"""Exercise same-Session Hitrace materialization publication contention."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import sys
import time
import uuid


_BARRIER_ENVIRONMENT_VARIABLE = "KAT_PAYLOAD_SMOKE_BARRIER"
_CLOCK_VALUE_OFFSET = 60
_CONTENDER_CLOCK_VALUES = (111_111, 222_222)
_EXPECTED_RELATIONS = ("clock_domain.parquet", "clock_snapshot.parquet")
_PACK_NAME = "payload-smoke"
_WORKFLOW_NAME = "summarize-hitrace-clock"


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(
            "usage: verify_payload_materialization_race.py "
            "KAT PACK SOURCE WORK_DIRECTORY",
            file=sys.stderr,
        )
        return 2

    cli = _regular_file(Path(argv[0]), "KAT executable")
    pack = _directory(Path(argv[1]), "Payload smoke PACK")
    source = _regular_file(Path(argv[2]), "Hitrace source")
    work = Path(argv[3])
    if work.exists() or work.is_symlink():
        raise RuntimeError("Payload materialization race work directory already exists")
    work.mkdir(parents=True)
    work = work.resolve(strict=True)
    data_home = work / "data-home"
    data_home.mkdir()

    environment = os.environ.copy()
    environment["KAT_DATA_HOME"] = str(data_home.resolve(strict=True))
    session_id = _create_session(cli, environment, work)

    seed_source = work / "session-seed.htrace"
    shutil.copyfile(source, seed_source)
    setup_response = _run(cli, pack, seed_source, environment, work, session_id)
    setup_session_id, setup_run_id = _successful_run(setup_response)
    if setup_session_id != session_id:
        raise RuntimeError("Setup Run did not use the explicitly created Session")
    session_root = data_home / "sessions" / session_id
    destination = session_root / "materializations" / source.stem
    contender_sources = _create_contender_sources(source, work)

    barrier = work / "barrier"
    barrier.mkdir()
    race_environment = environment.copy()
    race_environment[_BARRIER_ENVIRONMENT_VARIABLE] = str(
        barrier.resolve(strict=True)
    )
    processes: list[subprocess.Popen[bytes]] = []
    try:
        for contender in contender_sources:
            processes.append(
                _start_run(
                    cli, pack, contender, race_environment, work, session_id
                )
            )
        barrier_error: Exception | None = None
        try:
            ready = _wait_for_ready(barrier, processes)
            if _path_entry_exists(destination):
                raise RuntimeError(
                    "Hitrace materialization was published before both publishers were ready"
                )
            expected_destination = str(destination.resolve(strict=False))
            announced = {marker.read_text(encoding="utf-8") for marker in ready}
            if announced != {expected_destination}:
                raise RuntimeError("Publishers did not wait on the same destination")
        except Exception as error:
            barrier_error = error
        finally:
            _release_barrier(barrier)
        race_responses = _finish_runs(processes)
    except BaseException:
        _release_barrier_best_effort(barrier)
        _drain_or_kill(processes)
        raise
    if barrier_error is not None:
        raise RuntimeError(
            "Publishers did not reach the contention barrier"
        ) from barrier_error
    outcomes = sorted(
        marker.read_text(encoding="utf-8")
        for marker in barrier.glob("outcome-*")
        if marker.is_file()
    )
    if outcomes != ["published", "reused"]:
        raise RuntimeError("Contention did not produce one publisher and one reuser")

    race_runs = [_successful_run(response) for response in race_responses]
    if any(selected_session != session_id for selected_session, _ in race_runs):
        raise RuntimeError("Concurrent Runs did not stay in the selected Session")
    race_run_ids = {run_id for _, run_id in race_runs}
    if len(race_run_ids) != 2 or setup_run_id in race_run_ids:
        raise RuntimeError("Concurrent Runs did not receive distinct Run IDs")
    race_values = [
        _query_clock_value(cli, environment, work, session_id, run_id)
        for run_id in race_run_ids
    ]
    if (
        len(set(race_values)) != 1
        or race_values[0] not in _CONTENDER_CLOCK_VALUES
    ):
        raise RuntimeError("Concurrent Runs did not observe one winning materialization")

    published = _materialization_snapshot(destination)
    source.unlink()
    for contender in contender_sources:
        contender.unlink()
    reuse_response = _run(
        cli, pack, contender_sources[0], environment, work, session_id
    )
    reuse_session_id, reuse_run_id = _successful_run(reuse_response)
    if reuse_session_id != session_id:
        raise RuntimeError("Source-free reuse left the selected Session")
    if reuse_run_id in race_run_ids or reuse_run_id == setup_run_id:
        raise RuntimeError("Source-free reuse did not receive a distinct Run ID")
    if _materialization_snapshot(destination) != published:
        raise RuntimeError("Source-free reuse replaced the published materialization")
    reuse_value = _query_clock_value(
        cli, environment, work, session_id, reuse_run_id
    )
    if reuse_value != race_values[0]:
        raise RuntimeError("Source-free reuse did not observe the winning materialization")

    _validate_session_storage(
        session_root,
        expected_materializations={seed_source.stem, source.stem},
        expected_run_ids={setup_run_id, *race_run_ids, reuse_run_id},
    )
    return 0


def _create_contender_sources(source: Path, work: Path) -> list[Path]:
    original = source.read_bytes()
    contenders: list[Path] = []
    for index, clock_value in enumerate(_CONTENDER_CLOCK_VALUES, start=1):
        parent = work / f"publisher-{index}"
        parent.mkdir()
        contender = parent / source.name
        content = bytearray(original)
        struct.pack_into("<Q", content, _CLOCK_VALUE_OFFSET, clock_value)
        contender.write_bytes(content)
        contenders.append(contender)
    return contenders


def _run(
    cli: Path,
    pack: Path,
    source: Path,
    environment: dict[str, str],
    work: Path,
    session_id: str,
) -> dict[str, object]:
    completed = subprocess.run(
        _run_command(cli, pack, source, session_id),
        cwd=work,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    return _response(completed.returncode, completed.stdout, completed.stderr, "Run")


def _create_session(
    cli: Path,
    environment: dict[str, str],
    work: Path,
) -> str:
    completed = subprocess.run(
        (str(cli), "session", "create"),
        cwd=work,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    response = _response(
        completed.returncode,
        completed.stdout,
        completed.stderr,
        "Session create",
    )
    result = response.get("result")
    if type(result) is not dict or set(result) != {"session_id"}:
        raise RuntimeError("Session create did not return its exact result")
    return _uuid7(result["session_id"], "Session")


def _start_run(
    cli: Path,
    pack: Path,
    source: Path,
    environment: dict[str, str],
    work: Path,
    session_id: str,
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        _run_command(cli, pack, source, session_id),
        cwd=work,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _query_clock_value(
    cli: Path,
    environment: dict[str, str],
    work: Path,
    session_id: str,
    run_id: str,
) -> int:
    completed = subprocess.run(
        (
            str(cli),
            "query",
            "--session",
            session_id,
            "--run",
            run_id,
            "--sql",
            "SELECT clock_domain, clock_value FROM output.main",
        ),
        cwd=work,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    response = _response(
        completed.returncode, completed.stdout, completed.stderr, "Query"
    )
    result = response.get("result")
    if type(result) is not dict or result.get("format") != "ndjson":
        raise RuntimeError("Query did not return an NDJSON result")
    result_path = result.get("path")
    if type(result_path) is not str:
        raise RuntimeError("Query did not return its NDJSON path")
    records = _regular_file(Path(result_path), "Query result").read_text(
        encoding="utf-8"
    ).splitlines()
    if len(records) != 1:
        raise RuntimeError("Query did not return exactly one row")
    row = json.loads(records[0])
    if type(row) is not dict or row.get("clock_domain") != "boottime":
        raise RuntimeError("Query did not return the expected clock domain")
    clock_value = row.get("clock_value")
    if type(clock_value) is not int:
        raise RuntimeError("Query did not return an integer clock value")
    return clock_value


def _finish_runs(
    processes: list[subprocess.Popen[bytes]],
) -> list[dict[str, object]]:
    completed: list[tuple[int, bytes, bytes, str]] = []
    timeout_error: RuntimeError | None = None
    for index, process in enumerate(processes, start=1):
        label = f"publisher {index}"
        try:
            stdout, stderr = process.communicate(timeout=120)
        except subprocess.TimeoutExpired:
            process.kill()
            stdout, stderr = process.communicate()
            timeout_error = RuntimeError(
                f"{label} did not finish after barrier release"
            )
        completed.append((process.returncode, stdout, stderr, label))
    if timeout_error is not None:
        raise timeout_error
    return [_response(*result) for result in completed]


def _run_command(
    cli: Path, pack: Path, source: Path, session_id: str
) -> list[str]:
    command = [str(cli), "run", "--session", session_id]
    command.extend(
        (
            "--pack",
            _PACK_NAME,
            "--workflow",
            _WORKFLOW_NAME,
            "--pack-dir",
            str(pack),
            "--",
            "--trace-path",
            str(source),
        )
    )
    return command


def _wait_for_ready(
    barrier: Path, processes: list[subprocess.Popen[bytes]]
) -> list[Path]:
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        ready = sorted(barrier.glob("ready-*"))
        if len(ready) > 2:
            raise RuntimeError("Contention barrier received too many publishers")
        if len(ready) == 2 and all(
            marker.is_file() and marker.read_text(encoding="utf-8") for marker in ready
        ):
            return ready
        if any(process.poll() is not None for process in processes):
            raise RuntimeError("A publisher exited before both publishers were ready")
        time.sleep(0.02)
    raise RuntimeError("Timed out waiting for both materialization publishers")


def _release_barrier(barrier: Path) -> None:
    (barrier / "release").write_text("release", encoding="utf-8")


def _release_barrier_best_effort(barrier: Path) -> None:
    try:
        _release_barrier(barrier)
    except OSError:
        pass


def _drain_or_kill(processes: list[subprocess.Popen[bytes]]) -> None:
    for process in processes:
        try:
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                process.kill()
                process.communicate(timeout=5)
            except (OSError, ValueError, subprocess.TimeoutExpired):
                pass
        except (OSError, ValueError):
            pass


def _response(
    returncode: int, stdout: bytes, stderr: bytes, label: str
) -> dict[str, object]:
    terminal = stderr.decode(errors="replace").strip()
    if returncode != 0:
        raise RuntimeError(f"{label} exited with {returncode}: {terminal}")
    try:
        response = json.loads(stdout.decode("utf-8-sig"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise RuntimeError(
            f"{label} did not return a JSON Response: {terminal}"
        ) from error
    if type(response) is not dict:
        raise RuntimeError(f"{label} Response was not an object")
    if response.get("status") != "success":
        raise RuntimeError(f"{label} did not succeed: {response!r}; {terminal}")
    return response


def _successful_run(response: dict[str, object]) -> tuple[str, str]:
    result = response.get("result")
    if type(result) is not dict:
        raise RuntimeError("Successful Run did not contain a result object")
    outputs = result.get("outputs")
    if type(outputs) is not dict:
        raise RuntimeError("Successful Run did not publish the main Output")
    main_output = outputs.get("main")
    if type(main_output) is not dict:
        raise RuntimeError("Successful Run did not publish the main Output")
    if main_output.get("row_count") != 1:
        raise RuntimeError("Successful Run did not publish exactly one main row")
    session_id = _uuid7(result.get("session_id"), "Session")
    run_id = _uuid7(result.get("run_id"), "Run")
    return session_id, run_id


def _uuid7(value: object, label: str) -> str:
    if type(value) is not str:
        raise RuntimeError(f"{label} identity was not a string")
    try:
        identity = uuid.UUID(value)
    except ValueError:
        raise RuntimeError(f"{label} identity was invalid") from None
    if identity.version != 7 or str(identity) != value:
        raise RuntimeError(f"{label} identity was not a canonical UUIDv7")
    return value


def _materialization_snapshot(destination: Path) -> dict[str, str]:
    if not destination.is_dir() or _is_link(destination):
        raise RuntimeError("Hitrace materialization was not one ordinary directory")
    entries = sorted(destination.iterdir(), key=lambda path: path.name)
    if tuple(path.name for path in entries) != _EXPECTED_RELATIONS:
        raise RuntimeError("Hitrace materialization did not contain exact relations")
    snapshot: dict[str, str] = {}
    for path in entries:
        if not path.is_file() or _is_link(path):
            raise RuntimeError("Hitrace relation was not an ordinary file")
        snapshot[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
    return snapshot


def _validate_session_storage(
    session_root: Path,
    *,
    expected_materializations: set[str],
    expected_run_ids: set[str],
) -> None:
    if not session_root.is_dir() or _is_link(session_root):
        raise RuntimeError("Published Session root was not an ordinary directory")
    materializations = session_root / "materializations"
    if {path.name for path in materializations.iterdir()} != expected_materializations:
        raise RuntimeError("Materialization staging directories leaked")
    scratch = session_root / "scratch"
    if not scratch.is_dir() or any(scratch.iterdir()):
        raise RuntimeError("Run scratch candidates leaked")
    runs = session_root / "runs"
    actual_run_ids = {path.name for path in runs.iterdir()}
    if actual_run_ids != expected_run_ids:
        raise RuntimeError("Unpublished Run candidates leaked")
    for run_id in expected_run_ids:
        manifest = json.loads(
            (runs / run_id / "manifest.json").read_text(encoding="utf-8")
        )
        if (
            manifest.get("session_id") != session_root.name
            or manifest.get("run_id") != run_id
        ):
            raise RuntimeError("Published Run Manifest identities were inconsistent")


def _regular_file(path: Path, label: str) -> Path:
    resolved = path.resolve(strict=True)
    if not resolved.is_file() or _is_link(path):
        raise RuntimeError(f"{label} must be an ordinary file")
    return resolved


def _directory(path: Path, label: str) -> Path:
    resolved = path.resolve(strict=True)
    if not resolved.is_dir() or _is_link(path):
        raise RuntimeError(f"{label} must be an ordinary directory")
    return resolved


def _path_entry_exists(path: Path) -> bool:
    return os.path.lexists(path)


def _is_link(path: Path) -> bool:
    junction = getattr(path, "is_junction", None)
    return path.is_symlink() or (junction is not None and junction())


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
