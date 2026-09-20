#!/usr/bin/env python3
"""构建官方公共能力 SDK wheel 与校验摘要。"""

from __future__ import annotations

import argparse
from email import policy
from email.parser import BytesParser
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile


def validate_sdk_wheel_archive(path: Path, *, expected_version: str | None = None) -> str:
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        metadata_paths = [name for name in names if name.endswith(".dist-info/METADATA")]
        if len(metadata_paths) != 1:
            raise ValueError("SDK wheel must contain exactly one distribution")
        metadata = BytesParser(policy=policy.default).parsebytes(archive.read(metadata_paths[0]))
        version = metadata.get("Version")
        if metadata.get("Name") != "kat-sdk" or not version:
            raise ValueError("SDK wheel has unexpected distribution metadata")
        if expected_version is not None and version != expected_version:
            raise ValueError(f"SDK version mismatch: expected {expected_version}, got {version}")
        if path.name != f"kat_sdk-{version}-py3-none-any.whl":
            raise ValueError(f"Unexpected SDK wheel filename: {path.name}")
        allowed_root = {"kat_sdk", f"kat_sdk-{version}.dist-info"}
        if any(name.split("/")[0] not in allowed_root for name in names):
            raise ValueError("SDK wheel contains files outside its distribution")
        if any(name.startswith("kat_sdk/knowledge/libraries/") for name in names):
            raise ValueError("Library Markdown belongs in kat Skill references, not the SDK wheel")
        required = {
            "kat_sdk/__init__.py",
            "kat_sdk/providers/__init__.py",
            "kat_sdk/pack.toml",
            "kat_sdk/knowledge/index.md",
        }
        if missing := sorted(required - names):
            raise ValueError(f"SDK wheel is incomplete: {missing}")
        if any(
            "/tests/" in name or "/build/" in name or name.endswith("/sdk_build.py")
            for name in names
        ):
            raise ValueError("SDK wheel contains build or test files")
        manifest = tomllib.loads(archive.read("kat_sdk/pack.toml").decode("utf-8"))
        if set(manifest) != {"name", "title", "description", "owner"}:
            raise ValueError("SDK PACK manifest has unexpected fields")
        if any(not isinstance(value, str) or not value.strip() for value in manifest.values()):
            raise ValueError("SDK PACK manifest has empty fields")
        for name in names:
            if name.startswith("kat_sdk/knowledge/") and name.endswith(".md"):
                if not archive.read(name).decode("utf-8").strip():
                    raise ValueError(f"SDK knowledge is empty: {name}")
        wheel = BytesParser(policy=policy.default).parsebytes(
            archive.read(metadata_paths[0].removesuffix("METADATA") + "WHEEL")
        )
        if wheel.get("Root-Is-Purelib", "").lower() != "true" or wheel.get_all("Tag") != ["py3-none-any"]:
            raise ValueError("SDK wheel must contain pure Python and package resources")
    return version


def build_sdk_wheel(
    repository: Path,
    output: Path,
    *,
    expected_version: str | None = None,
) -> tuple[Path, Path]:
    repository = repository.resolve(strict=True)
    output = output.resolve()
    source_root = repository / "kat" / "sdk"
    if output == source_root or source_root in output.parents:
        raise ValueError("SDK wheel output must be outside the SDK source directory")
    if output.exists():
        raise ValueError(f"SDK wheel output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="sdk-build-", dir=output.parent) as temporary:
        temporary_root = Path(temporary)
        source = temporary_root / "source"
        shutil.copytree(
            repository / "kat" / "sdk",
            source,
            ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "build", "*.egg-info"),
        )
        built = temporary_root / "wheel"
        subprocess.run(
            [sys.executable, "-m", "build", "--wheel", "--outdir", str(built), str(source)],
            check=True,
            env={**os.environ, "SOURCE_DATE_EPOCH": "315532800", "PYTHONHASHSEED": "0"},
        )
        wheels = list(built.glob("*.whl"))
        if len(wheels) != 1:
            raise ValueError(f"Expected one SDK wheel, found {len(wheels)}")
        validate_sdk_wheel_archive(wheels[0], expected_version=expected_version)
        checksum = hashlib.sha256(wheels[0].read_bytes()).hexdigest()
        (built / (wheels[0].name + ".sha256")).write_text(
            f"{checksum}  {wheels[0].name}\n", encoding="ascii"
        )
        built.rename(output)
        wheel = output / wheels[0].name
    return wheel, output / (wheel.name + ".sha256")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-version")
    options = parser.parse_args()
    wheel, checksum = build_sdk_wheel(
        options.repository, options.output, expected_version=options.expected_version
    )
    print(wheel)
    print(checksum)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
