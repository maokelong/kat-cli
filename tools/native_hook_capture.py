#!/usr/bin/env python3
"""Capture a Native Hook trace while lightly exercising distributedcalc."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
from contextlib import contextmanager, redirect_stderr, redirect_stdout
from datetime import datetime
from pathlib import Path
from typing import Any, Callable, Iterator, Optional

BUNDLE = "ohos.samples.distributedcalc"
CALCULATION_COMPONENTS = ("C", "1", "0", "0", "*", "1", "0", "0", "=")
RESULT_COMPONENT = "expression"
EXPECTED_RESULT = "10000"


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--duration", type=int, default=30)
    parser.add_argument("--target")
    parser.add_argument("--hdc")
    parser.add_argument("--trace-streamer")
    parser.add_argument("--output-root", type=Path, default=Path("target/trace"))
    args = parser.parse_args()
    if args.duration <= 0:
        parser.error("--duration must be greater than zero")
    return args


def executable(value: Optional[str], default: str) -> str:
    found = shutil.which(value or default)
    if not found:
        raise RuntimeError(f"executable not found: {value or default}")
    return found


def hdc_run(hdc: str, target: str, *args: str, **kwargs) -> subprocess.CompletedProcess:
    kwargs.setdefault("check", True)
    return subprocess.run([hdc, "-t", target, *args], text=True, **kwargs)


def connected_targets(output: str) -> list[str]:
    targets = []
    for line in output.splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[2] == "Connected":
            targets.append(fields[0])
    return targets


def select_target(hdc: str, requested: Optional[str]) -> str:
    result = subprocess.run(
        [hdc, "list", "targets", "-v"], check=True, capture_output=True, text=True
    )
    targets = connected_targets(result.stdout)
    if requested:
        if requested not in targets:
            raise RuntimeError(f"target is not connected: {requested}")
        return requested
    if len(targets) != 1:
        raise RuntimeError(f"expected exactly one connected target, found {len(targets)}: {targets}")
    return targets[0]


def run_directory(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    base = datetime.now().strftime("%Y%m%d-%H%M%S")
    for suffix in [""] + [f"-{number:02d}" for number in range(1, 100)]:
        candidate = root / f"{base}{suffix}"
        try:
            candidate.mkdir()
            return candidate
        except FileExistsError:
            continue
    raise RuntimeError("cannot allocate a unique run directory")


def profiler_config() -> str:
    return f'''request_id: 1
session_config {{ buffers {{ pages: 131072 }} }}
plugin_configs {{
  plugin_name: "nativehook"
  sample_interval: 5000
  config_data {{
    save_file: false
    smb_pages: 16384
    process_name: "{BUNDLE}"
    string_compressed: true
    fp_unwind: false
    blocked: true
    callframe_compress: true
    record_accurately: true
    offline_symbolization: true
    startup_mode: false
    js_stack_report: 1
    max_stack_depth: 10
    max_js_stack_depth: 32
  }}
}}
'''


@contextmanager
def remote_profiler_config(
    hdc: str, target: str, run_dir: Path
) -> Iterator[str]:
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    local = run_dir / "hiprofiler.config"
    remote = f"/data/local/tmp/hiprofiler_{identifier}.config"
    sent = False
    try:
        local.write_text(profiler_config(), encoding="utf-8")
        hdc_run(hdc, target, "file", "send", str(local), remote)
        sent = True
        yield remote
    finally:
        local.unlink(missing_ok=True)
        if sent:
            hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def load_hypium() -> tuple[Callable[..., Any], Any]:
    try:
        from hypium import BY, UiDriver
    except ImportError as error:
        raise RuntimeError(
            "Hypium is not installed; install tools/requirements-native-hook-capture.txt"
        ) from error
    return UiDriver.connect, BY


def connect_hypium(
    target: str,
    log_path: Path,
    connector: Optional[Callable[..., Any]] = None,
    by: Any = None,
) -> tuple[Any, Any]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as log, redirect_stdout(log), redirect_stderr(log):
        if connector is None or by is None:
            connector, by = load_hypium()
        print(f"connect target: {target}")
        driver = connector(device_sn=target, report_path=str(log_path.parent))
    return driver, by


def run_hypium(driver: Any, by: Any, log_path: Path) -> None:
    with log_path.open("a", encoding="utf-8") as log, redirect_stdout(log), redirect_stderr(log):
        driver.stop_app(BUNDLE)
        driver.start_app(BUNDLE, "MainAbility", wait_time=1)
        for component_id in CALCULATION_COMPONENTS:
            component = driver.wait_for_component(by.id(component_id), timeout=3)
            component.click()
        result = driver.wait_for_component(by.id(RESULT_COMPONENT), timeout=3)
        actual = result.getText()
        print(f"calculator result: {actual}")
        if actual != EXPECTED_RESULT:
            raise RuntimeError(
                f"unexpected calculator result: {actual!r}, expected {EXPECTED_RESULT!r}"
            )


def close_hypium(driver: Any, log_path: Path) -> None:
    with log_path.open("a", encoding="utf-8") as log, redirect_stdout(log), redirect_stderr(log):
        driver.close()


def capture_failure(hdc: str, target: str, local_path: Path) -> None:
    remote = f"/data/local/tmp/hypium_failure_{datetime.now().strftime('%Y%m%d_%H%M%S_%f')}.png"
    captured = hdc_run(
        hdc,
        target,
        "shell",
        "snapshot_display",
        "-f",
        remote,
        capture_output=True,
        check=False,
    )
    if captured.returncode != 0:
        return
    received = hdc_run(
        hdc,
        target,
        "file",
        "recv",
        remote,
        str(local_path),
        capture_output=True,
        check=False,
    )
    if received.returncode == 0:
        hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def wait_for_profiler(
    hdc: str,
    target: str,
    remote: str,
    profiler: subprocess.Popen,
    timeout: float = 10,
) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline and profiler.poll() is None:
        probe = hdc_run(
            hdc,
            target,
            "shell",
            "stat",
            "-c",
            "%s",
            remote,
            capture_output=True,
            check=False,
        )
        if (
            probe.returncode == 0
            and probe.stdout.strip().isdigit()
            and int(probe.stdout.strip()) > 0
        ):
            return
        time.sleep(0.25)
    profiler.terminate()
    profiler.wait()
    raise RuntimeError("hiprofiler did not become ready within 10 seconds")


def convert_trace(
    streamer: str,
    trace: Path,
    database: Path,
    log_path: Path,
) -> None:
    with log_path.open("wb") as log:
        result = subprocess.run(
            [streamer, str(trace), "-e", str(database)],
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    if result.returncode != 0 or not database.is_file() or database.stat().st_size == 0:
        raise RuntimeError(f"trace_streamer failed with code {result.returncode}")


def main() -> int:
    args = arguments()
    hdc = executable(args.hdc, "hdc")
    streamer = executable(args.trace_streamer, "trace_streamer_windows.exe")
    target = select_target(hdc, args.target)

    run_dir = run_directory(args.output_root)
    trace = run_dir / "native_heap.htrace"
    database = run_dir / "trace.db"
    profiler_log = run_dir / "hiprofiler.log"
    hypium_log = run_dir / "hypium.log"
    streamer_log = run_dir / "trace-streamer.log"
    failure = run_dir / "failure.png"
    remote = f"/data/local/tmp/native_heap_{datetime.now().strftime('%Y%m%d_%H%M%S_%f')}.htrace"

    driver, by = connect_hypium(target, hypium_log)
    ui_error: Optional[Exception] = None
    try:
        with remote_profiler_config(hdc, target, run_dir) as config, profiler_log.open(
            "wb"
        ) as log:
            profiler_command = (
                f"hiprofiler_cmd -c {config} -o {remote} -t {args.duration} -s -k"
            )
            profiler = subprocess.Popen(
                [hdc, "-t", target, "shell", profiler_command],
                stdout=log,
                stderr=subprocess.STDOUT,
            )
            wait_for_profiler(hdc, target, remote, profiler)
            try:
                run_hypium(driver, by, hypium_log)
            except Exception as error:
                ui_error = error
                try:
                    capture_failure(hdc, target, failure)
                except Exception:
                    pass
                profiler.wait()
            else:
                profiler.wait()
            if profiler.returncode != 0:
                raise RuntimeError(f"hiprofiler exited with code {profiler.returncode}")
    finally:
        close_hypium(driver, hypium_log)
    if ui_error:
        raise RuntimeError(f"calculator interaction failed: {ui_error}")

    hdc_run(hdc, target, "file", "recv", remote, str(trace))
    hdc_run(hdc, target, "shell", "rm", "-f", remote)
    convert_trace(streamer, trace, database, streamer_log)
    print(run_dir.resolve())
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
