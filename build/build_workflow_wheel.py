#!/usr/bin/env python3
"""Build the single private Workflow Host wheel consumed by both payloads."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


WHEEL_NAME = "kat_workflow-0.1.0-py3-none-any.whl"


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def locked_uv_version(repository: Path) -> str:
    inputs = json.loads((repository / "build/runtime-inputs.json").read_text("utf-8"))
    version = inputs.get("uv", {}).get("version")
    if not isinstance(version, str) or not version:
        raise ValueError("runtime inputs do not lock a uv version")
    return version


def uv_version(uv: Path) -> str:
    completed = subprocess.run(
        [str(uv), "--version"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    match = re.fullmatch(r"uv ([0-9]+(?:\.[0-9]+){2})(?: .*)?", completed.stdout.strip())
    if match is None:
        raise ValueError(f"unexpected uv version output: {completed.stdout!r}")
    return match.group(1)


def validate_wheel(wheel: Path) -> None:
    if wheel.name != WHEEL_NAME or not wheel.is_file():
        raise ValueError(f"unexpected Workflow Host wheel: {wheel}")
    with zipfile.ZipFile(wheel) as archive:
        names = set(archive.namelist())
        required = {
            "kat/__init__.py",
            "_kat_runtime/__main__.py",
            "kat_workflow-0.1.0.dist-info/METADATA",
            "kat_workflow-0.1.0.dist-info/WHEEL",
        }
        missing = sorted(required - names)
        if missing:
            raise ValueError(f"Workflow Host wheel is incomplete: {missing}")
        wheel_metadata = archive.read(
            "kat_workflow-0.1.0.dist-info/WHEEL"
        ).decode("utf-8")
        if "Root-Is-Purelib: true" not in wheel_metadata:
            raise ValueError("Workflow Host wheel must be pure Python")


def build_workflow_wheel(repository: Path, uv: Path, output: Path) -> tuple[Path, Path]:
    repository = repository.resolve()
    uv = uv.resolve(strict=True)
    output = output.resolve()
    if output.exists():
        raise ValueError(f"wheel output already exists: {output}")
    expected_uv = locked_uv_version(repository)
    actual_uv = uv_version(uv)
    if actual_uv != expected_uv:
        raise ValueError(f"uv version mismatch: expected {expected_uv}, got {actual_uv}")

    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_root = Path(
        tempfile.mkdtemp(prefix="workflow-wheel-build-", dir=output.parent)
    )
    try:
        environment = os.environ.copy()
        environment.update(
            {
                "PYTHONHASHSEED": "0",
                "SOURCE_DATE_EPOCH": "315532800",
                "UV_NO_PROGRESS": "1",
            }
        )
        source = temporary_root / "source"
        shutil.copytree(
            repository / "kat/platform/workflow",
            source,
            ignore=shutil.ignore_patterns(
                "__pycache__", "*.pyc", "*.pyo", "build", "*.egg-info"
            ),
        )
        built = temporary_root / "built"
        built.mkdir()
        subprocess.run(
            [
                str(uv),
                "build",
                "--wheel",
                "--out-dir",
                str(built),
                str(source),
            ],
            check=True,
            env=environment,
        )
        wheels = list(built.glob("*.whl"))
        if len(wheels) != 1:
            raise ValueError(f"expected one Workflow Host wheel, found {len(wheels)}")
        validate_wheel(wheels[0])
        artifact = temporary_root / "artifact"
        artifact.mkdir()
        wheel = artifact / WHEEL_NAME
        shutil.copy2(wheels[0], wheel)
        checksum = artifact / f"{WHEEL_NAME}.sha256"
        checksum.write_text(
            f"{file_sha256(wheel)}  {WHEEL_NAME}\n", encoding="ascii"
        )
        artifact.rename(output)
    except BaseException:
        shutil.rmtree(temporary_root, ignore_errors=True)
        raise
    shutil.rmtree(temporary_root, ignore_errors=True)

    wheel = output / WHEEL_NAME
    checksum = output / f"{WHEEL_NAME}.sha256"
    return wheel, checksum


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--uv", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        options = parse_args(argv)
        build_workflow_wheel(options.repository, options.uv, options.output)
    except (OSError, ValueError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        print(f"Workflow Host wheel build failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
