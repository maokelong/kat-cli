#!/usr/bin/env python3
"""Build the single private Workflow Host wheel consumed by both payloads."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

import payload_builder


file_sha256 = payload_builder.file_sha256


def build_workflow_wheel(
    repository: Path,
    uv: Path | None,
    output: Path,
    *,
    expected_version: str,
    download_cache: Path | None = None,
) -> tuple[Path, Path]:
    repository = repository.resolve()
    output = output.resolve()
    if output.exists():
        raise ValueError(f"wheel output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_root = Path(
        tempfile.mkdtemp(prefix="workflow-wheel-build-", dir=output.parent)
    )
    try:
        locked_uv = payload_builder.load_uv_input(
            repository, "linux-x86_64", "Linux"
        )
        if uv is None:
            cache = (
                repository / "target/kat/downloads"
                if download_cache is None
                else download_cache
            ).resolve()
            archive = payload_builder.resolve_locked_asset(
                locked_uv.archive,
                None,
                cache,
                False,
            )
            extracted = temporary_root / "uv-archive"
            if locked_uv.layout.archive_format == "tar":
                payload_builder.safe_extract_tar(
                    archive,
                    extracted,
                    platform_label="Linux",
                )
            else:
                payload_builder.safe_extract_zip(archive, extracted)
            uv = payload_builder.find_uv(extracted, locked_uv.layout)
        else:
            uv = uv.resolve(strict=True)
        actual_uv = payload_builder.uv_version(uv)
        if actual_uv != locked_uv.version:
            raise ValueError(
                f"uv version mismatch: expected {locked_uv.version}, got {actual_uv}"
            )
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
        payload_builder.validate_workflow_wheel_archive(
            wheels[0],
            expected_version=expected_version,
        )
        artifact = temporary_root / "artifact"
        artifact.mkdir()
        wheel_name = wheels[0].name
        wheel = artifact / wheel_name
        shutil.copy2(wheels[0], wheel)
        checksum = artifact / f"{wheel_name}.sha256"
        checksum.write_text(
            f"{payload_builder.file_sha256(wheel)}  {wheel_name}\n", encoding="ascii"
        )
        artifact.rename(output)
    finally:
        shutil.rmtree(temporary_root, ignore_errors=True)

    wheel = output / wheel_name
    checksum = output / f"{wheel_name}.sha256"
    return wheel, checksum


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--download-cache", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-version", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        options = parse_args(argv)
        wheel, _ = build_workflow_wheel(
            options.repository,
            None,
            options.output,
            expected_version=options.expected_version,
            download_cache=options.download_cache,
        )
        metadata_version = payload_builder.validate_workflow_wheel_archive(
            wheel,
            expected_version=options.expected_version,
        )
        print(
            f"Workflow Host wheel: {wheel.name}; "
            f"METADATA Version: {metadata_version}"
        )
    except (OSError, ValueError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        print(f"Workflow Host wheel build failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
