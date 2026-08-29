#!/usr/bin/env python3
"""Capture a Native Hook trace while lightly exercising distributedcalc."""

from __future__ import annotations

import argparse
import json
import random
import re
import secrets
import shutil
import subprocess
import sys
import time
from contextlib import contextmanager
from datetime import datetime
from pathlib import Path
from typing import Iterator, Optional, Protocol

CALCULATOR_BUNDLE = "ohos.samples.distributedcalc"
NOTE_BUNDLE = "com.ohos.note"
NOTE_HOME_PAGE = "pages/MyNoteHome"
CALCULATION_COMPONENTS = ("C", "1", "0", "0", "*", "1", "0", "0", "=")
RESULT_COMPONENT = "expression"
EXPECTED_RESULT = "10000"
BOUNDS = re.compile(r"^\[(-?\d+),(-?\d+)\]\[(-?\d+),(-?\d+)\]$")


class ApplicationScenario(Protocol):
    name: str
    bundle: str

    def prepare(self, hdc: str, target: str, log_path: Path) -> None: ...

    def exercise(
        self,
        hdc: str,
        target: str,
        log_path: Path,
        profiler: subprocess.Popen,
    ) -> None: ...


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--duration", type=int, default=30)
    parser.add_argument("--target")
    parser.add_argument("--hdc")
    parser.add_argument("--trace-streamer")
    parser.add_argument("--output-root", type=Path, default=Path("target/trace"))
    parser.add_argument("--scenario", choices=SCENARIOS, default="calculator")
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
    kwargs.setdefault("encoding", "utf-8")
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
        [hdc, "list", "targets", "-v"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
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


def profiler_config(process_name: str) -> str:
    return f'''request_id: 1
session_config {{ buffers {{ pages: 131072 }} }}
plugin_configs {{
  plugin_name: "nativehook"
  sample_interval: 5000
  config_data {{
    save_file: false
    smb_pages: 16384
    process_name: "{process_name}"
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
    hdc: str, target: str, run_dir: Path, process_name: str
) -> Iterator[str]:
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    local = run_dir / "hiprofiler.config"
    remote = f"/data/local/tmp/hiprofiler_{identifier}.config"
    sent = False
    try:
        local.write_text(profiler_config(process_name), encoding="utf-8")
        hdc_run(hdc, target, "file", "send", str(local), remote)
        sent = True
        yield remote
    finally:
        local.unlink(missing_ok=True)
        if sent:
            hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def layout_nodes(node: dict) -> Iterator[dict]:
    yield node
    for child in node.get("children", []):
        yield from layout_nodes(child)


def component_attributes(layout: dict, component_id: str) -> dict:
    matches = [
        node.get("attributes", {})
        for node in layout_nodes(layout)
        if node.get("attributes", {}).get("id") == component_id
    ]
    if len(matches) != 1:
        raise RuntimeError(
            f"expected exactly one component with id {component_id!r}, found {len(matches)}"
        )
    return matches[0]


def attributes_bounds(
    attributes: dict, component_id: str
) -> tuple[int, int, int, int]:
    for name in ("visible", "enabled", "clickable"):
        if attributes.get(name) != "true":
            raise RuntimeError(f"component {component_id!r} is not {name}")
    match = BOUNDS.fullmatch(attributes.get("bounds", ""))
    if not match:
        raise RuntimeError(f"component {component_id!r} has invalid bounds")
    left, top, right, bottom = map(int, match.groups())
    if right <= left or bottom <= top:
        raise RuntimeError(f"component {component_id!r} has empty bounds")
    return left, top, right, bottom


def attributes_center(attributes: dict, component_id: str) -> tuple[int, int]:
    left, top, right, bottom = attributes_bounds(attributes, component_id)
    return (left + right) // 2, (top + bottom) // 2


def component_center(layout: dict, component_id: str) -> tuple[int, int]:
    return attributes_center(component_attributes(layout, component_id), component_id)


def calculator_button_centers(layout: dict) -> dict[str, tuple[int, int]]:
    centers: dict[str, tuple[int, int]] = {}
    for node in layout_nodes(layout):
        attributes = node.get("attributes", {})
        component_id = attributes.get("id", "")
        if (
            attributes.get("type") != "Button"
            or not component_id
            or any(
                attributes.get(name) != "true"
                for name in ("visible", "enabled", "clickable")
            )
        ):
            continue
        if component_id in centers:
            raise RuntimeError(
                f"duplicate clickable calculator button id: {component_id!r}"
            )
        centers[component_id] = attributes_center(attributes, component_id)
    if not centers:
        raise RuntimeError("no clickable calculator buttons found")
    return dict(sorted(centers.items()))


def note_action_centers(layout: dict) -> dict[str, tuple[int, int]]:
    attributes = component_attributes(layout, "searchInput")
    left, top, right, bottom = attributes_bounds(attributes, "searchInput")
    width = right - left
    y = (top + bottom) // 2
    return {
        "search:left": (left + width // 4, y),
        "search:center": ((left + right) // 2, y),
        "search:right": (right - width // 4, y),
    }


def note_home_is_visible(layout: dict) -> bool:
    return any(
        attributes.get("bundleName") == NOTE_BUNDLE
        and attributes.get("pagePath") == NOTE_HOME_PAGE
        for attributes in (
            node.get("attributes", {}) for node in layout_nodes(layout)
        )
    )


def calculator_result(layout: dict) -> str:
    return component_attributes(layout, RESULT_COMPONENT).get("text", "")


def logged_shell(hdc: str, target: str, command: str, log) -> None:
    result = hdc_run(
        hdc, target, "shell", command, capture_output=True, check=False
    )
    log.write(f"$ {command}\n{result.stdout or ''}{result.stderr or ''}")
    log.flush()
    if result.returncode != 0:
        raise RuntimeError(f"device command failed with code {result.returncode}: {command}")


def dump_layout(hdc: str, target: str, bundle: str, remote: str, log) -> dict:
    logged_shell(
        hdc,
        target,
        f"uitest dumpLayout -p {remote} -b {bundle}",
        log,
    )
    result = hdc_run(
        hdc, target, "shell", f"cat {remote}", capture_output=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"failed to read UiTest layout with code {result.returncode}")
    try:
        layout = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("UiTest returned invalid layout JSON") from error
    log.write(f"layout bytes: {len(result.stdout.encode('utf-8'))}\n")
    return layout


def wait_for_calculator_layout(hdc: str, target: str, remote: str, log) -> tuple[dict, dict]:
    deadline = time.monotonic() + 10
    last_error: Optional[Exception] = None
    while time.monotonic() < deadline:
        try:
            layout = dump_layout(hdc, target, CALCULATOR_BUNDLE, remote, log)
            centers = {
                component_id: component_center(layout, component_id)
                for component_id in dict.fromkeys(CALCULATION_COMPONENTS)
            }
            return layout, centers
        except RuntimeError as error:
            last_error = error
            time.sleep(0.2)
    raise RuntimeError(f"calculator page did not become ready: {last_error}")


def wait_for_note_home_layout(
    hdc: str, target: str, remote: str, log
) -> tuple[dict, dict]:
    deadline = time.monotonic() + 10
    last_error: Optional[Exception] = None
    while time.monotonic() < deadline:
        try:
            layout = dump_layout(hdc, target, NOTE_BUNDLE, remote, log)
            if not note_home_is_visible(layout):
                raise RuntimeError("note home page is not visible")
            return layout, note_action_centers(layout)
        except RuntimeError as error:
            last_error = error
            time.sleep(0.2)
    raise RuntimeError(f"note home page did not become ready: {last_error}")


def start_application(hdc: str, target: str, bundle: str, ability: str, log) -> None:
    logged_shell(hdc, target, f"aa force-stop {bundle}", log)
    logged_shell(hdc, target, f"aa start -b {bundle} -a {ability}", log)


def start_calculator(hdc: str, target: str, log_path: Path) -> None:
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    remote = f"/data/local/tmp/distributedcalc_{identifier}.json"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with log_path.open("w", encoding="utf-8") as log:
            start_application(
                hdc, target, CALCULATOR_BUNDLE, "MainAbility", log
            )
            wait_for_calculator_layout(hdc, target, remote, log)
    finally:
        hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def click_calculation(hdc: str, target: str, centers: dict, log) -> None:
    for component_id in CALCULATION_COMPONENTS:
        x, y = centers[component_id]
        logged_shell(hdc, target, f"uitest uiInput click {x} {y}", log)


def prepare_calculator(hdc: str, target: str, log_path: Path) -> None:
    start_calculator(hdc, target, log_path)
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    remote = f"/data/local/tmp/distributedcalc_{identifier}.json"
    try:
        with log_path.open("a", encoding="utf-8") as log:
            _, centers = wait_for_calculator_layout(hdc, target, remote, log)
            click_calculation(hdc, target, centers, log)

            deadline = time.monotonic() + 3
            actual = ""
            while time.monotonic() < deadline:
                actual = calculator_result(
                    dump_layout(hdc, target, CALCULATOR_BUNDLE, remote, log)
                )
                if actual == EXPECTED_RESULT:
                    log.write(f"calculator result: {actual}\n")
                    return
                time.sleep(0.2)
            raise RuntimeError(
                f"unexpected calculator result: {actual!r}, expected {EXPECTED_RESULT!r}"
            )
    finally:
        hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def exercise_calculator(
    hdc: str,
    target: str,
    log_path: Path,
    profiler: subprocess.Popen,
    seed: Optional[int] = None,
) -> int:
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    remote = f"/data/local/tmp/distributedcalc_{identifier}.json"
    click_count = 0
    try:
        with log_path.open("a", encoding="utf-8") as log:
            centers = calculator_button_centers(
                dump_layout(hdc, target, CALCULATOR_BUNDLE, remote, log)
            )
            button_ids = tuple(centers)
            actual_seed = secrets.randbits(64) if seed is None else seed
            generator = random.Random(actual_seed)
            log.write(f"random seed: {actual_seed}\n")
            log.write(f"random buttons: {', '.join(button_ids)}\n")
            log.flush()
            try:
                while profiler.poll() is None:
                    component_id = generator.choice(button_ids)
                    x, y = centers[component_id]
                    try:
                        logged_shell(
                            hdc, target, f"uitest uiInput click {x} {y}", log
                        )
                    except RuntimeError:
                        if profiler.poll() is not None:
                            break
                        raise
                    click_count += 1
            finally:
                log.write(f"random click count: {click_count}\n")
                log.flush()
        return click_count
    finally:
        hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def prepare_note(
    hdc: str, target: str, log_path: Path
) -> dict[str, tuple[int, int]]:
    identifier = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    remote = f"/data/local/tmp/note_{identifier}.json"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with log_path.open("w", encoding="utf-8") as log:
            start_application(hdc, target, NOTE_BUNDLE, "MainAbility", log)
            _, centers = wait_for_note_home_layout(hdc, target, remote, log)
            log.write(f"note safe actions: {', '.join(centers)}\n")
            return centers
    finally:
        hdc_run(hdc, target, "shell", "rm", "-f", remote, check=False)


def exercise_note(
    hdc: str,
    target: str,
    log_path: Path,
    profiler: subprocess.Popen,
    centers: dict[str, tuple[int, int]],
    seed: Optional[int] = None,
) -> int:
    click_count = 0
    with log_path.open("a", encoding="utf-8") as log:
        actual_seed = secrets.randbits(64) if seed is None else seed
        generator = random.Random(actual_seed)
        log.write(f"random seed: {actual_seed}\n")
        log.flush()
        try:
            while profiler.poll() is None:
                action_id = generator.choice(tuple(centers))
                x, y = centers[action_id]
                log.write(f"note random action: {action_id}\n")
                try:
                    logged_shell(
                        hdc, target, f"uitest uiInput click {x} {y}", log
                    )
                except RuntimeError:
                    if profiler.poll() is not None:
                        break
                    raise
                click_count += 1
                time.sleep(0.2)
        finally:
            log.write(f"random click count: {click_count}\n")
            log.flush()
    return click_count


class CalculatorScenario:
    name = "calculator"
    bundle = CALCULATOR_BUNDLE

    def prepare(self, hdc: str, target: str, log_path: Path) -> None:
        prepare_calculator(hdc, target, log_path)

    def exercise(
        self,
        hdc: str,
        target: str,
        log_path: Path,
        profiler: subprocess.Popen,
    ) -> None:
        exercise_calculator(hdc, target, log_path, profiler)


class NoteScenario:
    name = "note"
    bundle = NOTE_BUNDLE

    def __init__(self) -> None:
        self.centers: Optional[dict[str, tuple[int, int]]] = None

    def prepare(self, hdc: str, target: str, log_path: Path) -> None:
        self.centers = prepare_note(hdc, target, log_path)

    def exercise(
        self,
        hdc: str,
        target: str,
        log_path: Path,
        profiler: subprocess.Popen,
    ) -> None:
        if self.centers is None:
            raise RuntimeError("note scenario was not prepared")
        exercise_note(hdc, target, log_path, profiler, self.centers)


SCENARIOS: dict[str, ApplicationScenario] = {
    CalculatorScenario.name: CalculatorScenario(),
    NoteScenario.name: NoteScenario(),
}


def capture_failure(hdc: str, target: str, local_path: Path) -> None:
    remote = f"/data/local/tmp/uitest_failure_{datetime.now().strftime('%Y%m%d_%H%M%S_%f')}.png"
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


def exercise_scenario(
    scenario: ApplicationScenario,
    hdc: str,
    target: str,
    log_path: Path,
    failure_path: Path,
    profiler: subprocess.Popen,
) -> Optional[str]:
    try:
        scenario.exercise(hdc, target, log_path, profiler)
        return None
    except Exception as error:
        warning = f"{scenario.name} interaction warning: {error}"
        with log_path.open("a", encoding="utf-8") as log:
            log.write(f"{warning}\n")
        print(f"warning: {warning}", file=sys.stderr)
        try:
            capture_failure(hdc, target, failure_path)
        except Exception:
            pass
        return warning


def main() -> int:
    args = arguments()
    scenario = SCENARIOS[args.scenario]
    hdc = executable(args.hdc, "hdc")
    streamer = executable(args.trace_streamer, "trace_streamer_windows.exe")
    target = select_target(hdc, args.target)

    run_dir = run_directory(args.output_root)
    trace = run_dir / "native_heap.htrace"
    database = run_dir / "trace.db"
    profiler_log = run_dir / "hiprofiler.log"
    uitest_log = run_dir / "uitest.log"
    streamer_log = run_dir / "trace-streamer.log"
    failure = run_dir / "failure.png"
    remote = f"/data/local/tmp/native_heap_{datetime.now().strftime('%Y%m%d_%H%M%S_%f')}.htrace"

    try:
        scenario.prepare(hdc, target, uitest_log)
    except Exception as error:
        try:
            capture_failure(hdc, target, failure)
        except Exception:
            pass
        raise RuntimeError(
            f"{scenario.name} startup validation failed: {error}"
        ) from error

    with remote_profiler_config(
        hdc, target, run_dir, scenario.bundle
    ) as config, profiler_log.open("wb") as log:
        profiler_command = (
            f"hiprofiler_cmd -c {config} -o {remote} -t {args.duration} -s -k"
        )
        profiler = subprocess.Popen(
            [hdc, "-t", target, "shell", profiler_command],
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        wait_for_profiler(hdc, target, remote, profiler)
        exercise_scenario(
            scenario, hdc, target, uitest_log, failure, profiler
        )
        profiler.wait()
        if profiler.returncode != 0:
            raise RuntimeError(f"hiprofiler exited with code {profiler.returncode}")

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
