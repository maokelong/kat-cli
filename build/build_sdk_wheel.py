#!/usr/bin/env python3
"""构建官方公共能力 SDK wheel 与校验摘要。"""

from __future__ import annotations

import argparse
import ast
from email import policy
from email.parser import BytesParser
import hashlib
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile


def validate_declaration_guides(
    archive: zipfile.ZipFile, names: set[str]
) -> None:
    for category in ("providers", "workflows"):
        prefix = f"kat_sdk/{category}/"
        for name in sorted(names):
            if (
                not name.startswith(prefix)
                or not name.endswith(".py")
                or PurePosixPath(name).name.startswith("_")
            ):
                continue
            try:
                module = ast.parse(
                    archive.read(name).decode("utf-8"), filename=name
                )
            except (SyntaxError, UnicodeDecodeError) as error:
                raise ValueError(f"SDK module cannot be inspected: {name}") from error
            for member in module.body:
                if not isinstance(
                    member, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)
                ):
                    continue
                for decorator in member.decorator_list:
                    if not isinstance(decorator, ast.Call):
                        continue
                    function = decorator.func
                    decorator_name = (
                        function.id
                        if isinstance(function, ast.Name)
                        else function.attr
                        if isinstance(function, ast.Attribute)
                        else None
                    )
                    if decorator_name not in {"provider", "workflow"}:
                        continue
                    guide = next(
                        (
                            keyword.value
                            for keyword in decorator.keywords
                            if keyword.arg == "guide"
                        ),
                        None,
                    )
                    if guide is None:
                        if category == "providers":
                            raise ValueError(
                                f"SDK Provider must declare a Runtime Guide: {name}"
                            )
                        continue
                    try:
                        reference = ast.literal_eval(guide)
                    except (ValueError, TypeError) as error:
                        raise ValueError(
                            f"SDK declaration has an invalid Runtime Guide: {name}"
                        ) from error
                    if reference is None and category == "workflows":
                        continue
                    relative = (
                        PurePosixPath(reference)
                        if isinstance(reference, str)
                        else PurePosixPath()
                    )
                    target = f"kat_sdk/knowledge/{relative.as_posix()}"
                    if (
                        not isinstance(reference, str)
                        or relative.is_absolute()
                        or not relative.parts
                        or ".." in relative.parts
                        or relative.parts[0] != category
                        or relative.suffix != ".md"
                        or target not in names
                        or not archive.read(target).decode("utf-8").strip()
                    ):
                        raise ValueError(
                            f"SDK declaration Runtime Guide is missing or invalid: "
                            f"{name}: {reference!r}"
                        )


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
        if any(name.startswith("kat_sdk/knowledge/helpers/") for name in names):
            raise ValueError("Library Markdown belongs in kat Skill references, not the SDK wheel")
        if any(
            name == "kat_sdk/docs"
            or name.startswith("kat_sdk/docs/")
            or name == "kat_sdk/api.md"
            or name == "kat_sdk/reference"
            or name.startswith("kat_sdk/reference/")
            or (
                name.startswith("kat_sdk/knowledge/")
                and name.endswith(".api.md")
            )
            for name in names
        ):
            raise ValueError("SDK API documentation must stay outside the wheel")
        required = {
            "kat_sdk/__init__.py",
            "kat_sdk/providers/__init__.py",
            "kat_sdk/pack.toml",
        }
        if missing := sorted(required - names):
            raise ValueError(f"SDK wheel is incomplete: {missing}")
        if any(
            "/tests/" in name or "/build/" in name or name.endswith("/sdk_build.py")
            for name in names
        ):
            raise ValueError("SDK wheel contains build or test files")
        validate_declaration_guides(archive, names)
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
