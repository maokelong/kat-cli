#!/usr/bin/env python3
"""构建完整的 Linux x86_64 KAT Platform Payload。"""

from __future__ import annotations

import argparse
import os
import re
import stat
import subprocess
import sys
import tarfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

import payload_builder


PLATFORM = "linux-x86_64"
PLATFORM_SPEC = payload_builder.PlatformSpec(
    key=PLATFORM,
    label="Linux",
    managed_python_fields=("linux-x86_64-gnu", "linux", "gnu"),
    managed_python_launcher_glob="*/bin/python3",
    managed_python_root_parents=2,
    private_python_parts=("python", "bin", "python3"),
    copy_uv_links=False,
    site_packages_globs=("lib/python*/site-packages",),
    prune_paths=(("share", "terminfo"),),
    private_bin_parts=("bin",),
    private_bin_keep_prefix="python",
    cli_filename="kat",
    cargo_environment=(),
    native_wheel_platform_tag="manylinux_2_28_x86_64",
    native_extension_suffix=".so",
    native_wheel_compatibility="manylinux_2_28",
)
GLIBC_VERSION = re.compile(r"GLIBC_(\d+)\.(\d+)")
@dataclass(frozen=True)
class LinuxInputs(payload_builder.CommonInputs):
    minimum_glibc: tuple[int, int]


@dataclass(frozen=True)
class BuildOptions:
    repository: Path
    output: Path
    download_cache: Path
    python_archive: Path | None
    uv_archive: Path | None
    wheelhouse: Path | None
    cargo: str
    readelf: str
    offline: bool
    workflow_wheel: Path | None = None


def load_inputs(repository: Path) -> LinuxInputs:
    common, platform = payload_builder.load_common_inputs(
        repository,
        platform_name=PLATFORM,
        rust_target="x86_64-unknown-linux-gnu",
        platform_label="Linux",
    )
    glibc_text = platform.get("minimumGlibc")
    match = re.fullmatch(r"(\d+)\.(\d+)", str(glibc_text))
    if match is None:
        raise ValueError("Linux minimum glibc must be a major.minor version")
    return LinuxInputs(
        python_version=common.python_version,
        rust_target=common.rust_target,
        python_archive=common.python_archive,
        uv=common.uv,
        requirements_lock=common.requirements_lock,
        minimum_glibc=(int(match.group(1)), int(match.group(2))),
    )


def parse_glibc_versions(output: str) -> set[tuple[int, int]]:
    return {(int(major), int(minor)) for major, minor in GLIBC_VERSION.findall(output)}


def elf_files(root: Path) -> Iterable[Path]:
    seen: set[Path] = set()
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        resolved = path.resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        with path.open("rb") as source:
            if source.read(4) == b"\x7fELF":
                yield path


def verify_native_baseline(root: Path, readelf: str, maximum: tuple[int, int]) -> None:
    native_files = list(elf_files(root))
    if not native_files or root / "kat" not in native_files:
        raise ValueError("Linux payload must contain an ELF KAT CLI")
    environment = dict(os.environ)
    environment["LC_ALL"] = "C"
    for path in native_files:
        header = subprocess.run(
            [readelf, "--file-header", str(path)],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        ).stdout
        if "Advanced Micro Devices X86-64" not in header:
            raise ValueError(f"Linux payload contains a non-x86_64 ELF file: {path}")
        versions = subprocess.run(
            [readelf, "--version-info", str(path)],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        ).stdout
        too_new = sorted(
            version for version in parse_glibc_versions(versions) if version > maximum
        )
        if too_new:
            rendered = ", ".join(f"GLIBC_{major}.{minor}" for major, minor in too_new)
            raise ValueError(
                f"{path} exceeds the glibc {maximum[0]}.{maximum[1]} baseline: {rendered}"
            )


def assert_payload_shape(payload: Path) -> None:
    entries = {path.name for path in payload.iterdir()}
    if entries != {"kat", "python"}:
        raise ValueError(f"Linux payload root must contain only kat and python/, got {entries}")
    cli = payload / "kat"
    python = payload_builder.private_python(payload, PLATFORM_SPEC)
    if not cli.is_file() or not python.is_file():
        raise ValueError("Linux payload is missing kat or python/bin/python3")
    if os.name != "nt" and not os.access(cli, os.X_OK):
        raise ValueError("Linux payload KAT CLI is not executable")
    terminfo = payload / "python/share/terminfo"
    if terminfo.exists():
        raise ValueError("Linux payload contains bundled terminfo")
    payload_builder.assert_no_build_artifacts(payload, PLATFORM_SPEC)


class LinuxAdapter:
    spec = PLATFORM_SPEC

    def __init__(self, *, readelf: str) -> None:
        self.readelf = readelf

    def require_builder(self) -> None:
        payload_builder.require_builder_python(self.spec.label)

    def load_inputs(self, repository: Path) -> LinuxInputs:
        return load_inputs(repository)

    def extra_input_paths(self) -> Iterable[tuple[str, Path | None]]:
        return ()

    def resolve_extra_inputs(
        self,
        inputs: LinuxInputs,
        cache: Path,
        offline: bool,
    ) -> None:
        return None

    def finalize_payload(
        self,
        payload: Path,
        temporary_root: Path,
        inputs: LinuxInputs,
        extra_inputs: None,
    ) -> None:
        cli = payload / "kat"
        cli.chmod(cli.stat().st_mode | stat.S_IXUSR)
        verify_native_baseline(payload, self.readelf, inputs.minimum_glibc)

    def assert_payload_shape(self, payload: Path) -> None:
        assert_payload_shape(payload)


def build_payload(options: BuildOptions) -> Path:
    return payload_builder.build_payload(
        options, LinuxAdapter(readelf=options.readelf)
    )


def parse_args(argv: list[str] | None = None) -> BuildOptions:
    repository_default = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Build the complete Linux x86_64 KAT Platform Payload"
    )
    parser.add_argument("--repository", type=Path, default=repository_default)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--download-cache", type=Path)
    parser.add_argument("--python-archive", type=Path)
    parser.add_argument("--uv-archive", type=Path)
    parser.add_argument("--wheelhouse", type=Path)
    parser.add_argument("--workflow-wheel", type=Path)
    parser.add_argument(
        "--cargo",
        default="cargo",
        help="Cargo executable used by a native glibc 2.28 build environment",
    )
    parser.add_argument("--readelf", default="readelf")
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args(argv)
    repository = args.repository.resolve()
    output = args.output or repository / "target/kat/payloads/linux-x86_64"
    download_cache = args.download_cache or repository / "target/kat/downloads"
    workflow_wheel = args.workflow_wheel or payload_builder.find_workflow_wheel(
        repository / "target/kat/workflow-wheel"
    )
    return BuildOptions(
        repository=repository,
        output=output,
        download_cache=download_cache,
        python_archive=args.python_archive,
        uv_archive=args.uv_archive,
        wheelhouse=args.wheelhouse,
        cargo=args.cargo,
        readelf=args.readelf,
        offline=args.offline,
        workflow_wheel=workflow_wheel,
    )


def main(argv: list[str] | None = None) -> int:
    try:
        output = build_payload(parse_args(argv))
    except (
        OSError,
        ValueError,
        subprocess.CalledProcessError,
        tarfile.TarError,
        zipfile.BadZipFile,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
