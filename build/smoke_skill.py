#!/usr/bin/env python3
"""Exercise a relocated final Skill through Import, Run, Query, and kat test."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path
from typing import Any


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="\n")


def create_external_pack(pack: Path) -> None:
    write_text(
        pack / "pack.toml",
        'name = "payload-smoke"\n'
        'title = "Payload smoke"\n'
        'description = "Exercise the final private Host"\n'
        'owner = "KAT Release"\n',
    )
    write_text(
        pack / "workflows" / "sum_values.py",
        '''import kat


@kat.workflow(
    name="sum-values",
    title="Sum values",
    required_tables=["events"],
    parameters={},
)
def sum_values(ctx: kat.Context):
    """Return one bounded aggregate used by the payload smoke."""
    return {"totals": ctx.sql("SELECT SUM(value) AS total FROM events")}
''',
    )


def create_input(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(path) as connection:
        connection.executescript(
            "CREATE TABLE events(value INTEGER);"
            "INSERT INTO events VALUES (2), (3);"
        )


def protected_digest(skill: Path) -> dict[str, str]:
    roots = [
        skill / "SKILL.md",
        skill / "agents",
        skill / "references",
        skill / "assets" / "packs",
    ]
    result: dict[str, str] = {}
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.rglob("*"))
        for path in paths:
            if path.is_symlink():
                raise RuntimeError(f"Skill source contains a symlink: {path}")
            if path.is_file():
                relative = path.relative_to(skill).as_posix()
                if "__pycache__" in path.parts or path.suffix in {".pyc", ".pyo"}:
                    raise RuntimeError(f"Skill source contains Python cache: {relative}")
                result[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def run_json(command: list[str], environment: dict[str, str], cwd: Path) -> Any:
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        cwd=cwd,
        env=environment,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {command}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    try:
        response = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"KAT stdout is not one JSON document: {completed.stdout}") from error
    if response.get("status") != "success":
        raise RuntimeError(f"KAT returned a failure response: {response}")
    return response["result"]


def require_string(value: Any, key: str) -> str:
    if isinstance(value, dict):
        candidate = value.get(key)
        if isinstance(candidate, str):
            return candidate
        for nested in value.values():
            try:
                return require_string(nested, key)
            except KeyError:
                pass
    if isinstance(value, list):
        for nested in value:
            try:
                return require_string(nested, key)
            except KeyError:
                pass
    raise KeyError(key)


def smoke(skill_source: Path, platform: str, work: Path) -> dict[str, Any]:
    skill_source = skill_source.resolve(strict=True)
    work = work.resolve()
    if work.exists():
        raise ValueError(f"smoke work directory already exists: {work}")
    relocated = work / "arbitrary cwd" / "relocated KAT"
    relocated.parent.mkdir(parents=True)
    shutil.copytree(skill_source, relocated)

    target = "windows-x86_64" if platform == "windows" else "linux-x86_64"
    binary = "kat.exe" if platform == "windows" else "kat"
    kat = relocated / "scripts" / "targets" / target / binary
    if not kat.is_file():
        raise ValueError(f"selected payload entry is missing: {kat}")

    pack = work / "external source" / "payload-smoke"
    database = work / "inputs" / "events.db"
    dataset = work / "state" / "dataset"
    create_external_pack(pack)
    create_input(database)
    before = protected_digest(relocated)

    writable = work / "writable home"
    temporary = work / "temporary"
    cwd = work / "unrelated cwd"
    for directory in (writable, temporary, cwd):
        directory.mkdir(parents=True)
    environment = os.environ.copy()
    environment.update(
        {
            "APPDATA": str(writable),
            "HOME": str(writable),
            "LOCALAPPDATA": str(writable),
            "PATH": "",
            "PYTHONHOME": str(work / "poison-home"),
            "PYTHONPATH": str(work / "poison-path"),
            "PYTHONUSERBASE": str(work / "poison-user"),
            "TEMP": str(temporary),
            "TMP": str(temporary),
            "TMPDIR": str(temporary),
            "XDG_DATA_HOME": str(writable),
        }
    )

    imported = run_json(
        [
            str(kat),
            "import",
            "--dataset",
            str(dataset),
            "trace-streamer",
            "--database",
            str(database),
        ],
        environment,
        cwd,
    )
    canonical_dataset = require_string(imported, "path")
    executed = run_json(
        [
            str(kat),
            "run",
            "--pack",
            "payload-smoke",
            "--workflow",
            "sum-values",
            "--dataset",
            canonical_dataset,
            "--pack-dir",
            str(pack),
        ],
        environment,
        cwd,
    )
    run_id = require_string(executed, "run_id")
    queried = run_json(
        [
            str(kat),
            "query",
            "--run",
            run_id,
            "--sql",
            "SELECT total FROM output.totals",
        ],
        environment,
        cwd,
    )
    if "5" not in json.dumps(queried, separators=(",", ":")):
        raise RuntimeError(f"unexpected bounded Query result: {queried}")

    tested = []
    for pack_name in ("kat-kernel", "kat-openharmony-demo"):
        run_json([str(kat), "test", "--pack", pack_name], environment, cwd)
        tested.append(pack_name)

    if protected_digest(relocated) != before:
        raise RuntimeError("Skill definition or Bundled PACK source changed during smoke")
    return {
        "dataset": canonical_dataset,
        "platform": platform,
        "query_total": 5,
        "run_id": run_id,
        "skill_and_pack_source_unchanged": True,
        "tested_packs": tested,
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skill", required=True, type=Path)
    parser.add_argument("--platform", required=True, choices=("linux", "windows"))
    parser.add_argument("--work", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        options = parse_args(argv)
        summary = smoke(options.skill, options.platform, options.work)
    except (KeyError, OSError, RuntimeError, ValueError, sqlite3.Error) as error:
        print(f"final Skill smoke failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
