#!/usr/bin/env python3
"""在当前原生平台构建私有 kat-datasource wheel。"""

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

PLATFORMS = {
    "linux-x86_64": "manylinux_2_28",
    "windows-x86_64": None,
}


def build_datasource_wheel(
    repository: Path,
    python: Path,
    output: Path,
    *,
    platform: str,
    expected_version: str,
    cargo_target_dir: Path,
) -> tuple[Path, Path]:
    try:
        compatibility = PLATFORMS[platform]
    except KeyError:
        raise ValueError(f"unsupported Datasource wheel platform: {platform}") from None
    repository = repository.resolve()
    python = python.resolve(strict=True)
    output = output.resolve()
    cargo_target_dir = cargo_target_dir.resolve()
    if output.exists():
        raise ValueError(f"Datasource wheel output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_root = Path(
        tempfile.mkdtemp(prefix="datasource-wheel-build-", dir=output.parent)
    )
    try:
        built = temporary_root / "built"
        built.mkdir()
        # Builder 已运行在目标原生平台；显式 --target 会让 maturin 把 Windows
        # python.exe 当作 cross-build interpreter，并因文件名不含版本而拒绝它。
        command = [
            str(python),
            "-m",
            "maturin",
            "build",
            "--release",
            "--locked",
            "--manifest-path",
            str(repository / "kat/platform/datasource/Cargo.toml"),
            "--interpreter",
            str(python),
            "--out",
            str(built),
            "--target-dir",
            str(cargo_target_dir),
        ]
        if compatibility is not None:
            command.extend(("--compatibility", compatibility))
        environment = os.environ.copy()
        environment.pop("CARGO_TARGET_DIR", None)
        environment.pop("CARGO_BUILD_TARGET_DIR", None)
        environment.update(
            {
                "PYTHONHASHSEED": "0",
                "SOURCE_DATE_EPOCH": "315532800",
            }
        )
        subprocess.run(
            command,
            check=True,
            cwd=repository,
            env=environment,
        )
        wheels = list(built.glob("kat_datasource-*.whl"))
        if len(wheels) != 1:
            raise ValueError(
                f"expected one Datasource wheel, found {len(wheels)}"
            )
        payload_builder.validate_datasource_wheel_archive(
            wheels[0],
            expected_version=expected_version,
            platform=platform,
        )
        artifact = temporary_root / "artifact"
        artifact.mkdir()
        wheel_name = wheels[0].name
        wheel = artifact / wheel_name
        shutil.copy2(wheels[0], wheel)
        checksum = artifact / f"{wheel_name}.sha256"
        checksum.write_text(
            f"{file_sha256(wheel)}  {wheel_name}\n",
            encoding="ascii",
        )
        artifact.rename(output)
    finally:
        shutil.rmtree(temporary_root, ignore_errors=True)

    return output / wheel_name, output / f"{wheel_name}.sha256"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    repository_default = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=repository_default)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", choices=tuple(PLATFORMS), required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--cargo-target-dir", type=Path)
    arguments = parser.parse_args(argv)
    if arguments.cargo_target_dir is None:
        arguments.cargo_target_dir = (
            arguments.repository
            / "target"
            / "kat"
            / "cargo"
            / arguments.platform
        )
    return arguments


def main(argv: list[str] | None = None) -> int:
    try:
        options = parse_args(argv)
        wheel, _ = build_datasource_wheel(
            options.repository,
            options.python,
            options.output,
            platform=options.platform,
            expected_version=options.expected_version,
            cargo_target_dir=options.cargo_target_dir,
        )
        print(wheel)
    except (
        OSError,
        ValueError,
        subprocess.CalledProcessError,
        zipfile.BadZipFile,
    ) as error:
        print(f"Datasource wheel build failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
