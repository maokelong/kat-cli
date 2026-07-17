#!/usr/bin/env python3
"""构建完整的 Linux x86_64 KAT Platform Payload。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


PLATFORM = "linux-x86_64"
PYTHON_ENVIRONMENT_VARIABLES = (
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONUSERBASE",
    "PYTHONINSPECT",
    "PYTHONSTARTUP",
    "PYTHONWARNINGS",
    "VIRTUAL_ENV",
)
LOCK_LINE = re.compile(
    r"^(?P<name>[A-Za-z0-9_.-]+)==(?P<version>[^\s]+) "
    r"--hash=sha256:(?P<sha256>[0-9a-f]{64})$"
)
GLIBC_VERSION = re.compile(r"GLIBC_(\d+)\.(\d+)")
APPROVED_REQUIREMENTS = {
    "click": "8.4.2",
    "cloudpickle": "3.1.2",
    "datafusion": "54.0.0",
    "iniconfig": "2.3.0",
    "packaging": "26.2",
    "pluggy": "1.6.0",
    "pyarrow": "24.0.0",
    "pygments": "2.20.0",
    "pytest": "9.1.1",
}


@dataclass(frozen=True)
class LockedAsset:
    filename: str
    url: str
    sha256: str

    @classmethod
    def from_json(cls, value: Any, description: str) -> "LockedAsset":
        if not isinstance(value, dict) or set(value) != {"filename", "url", "sha256"}:
            raise ValueError(f"{description} must lock filename, URL, and SHA-256")
        asset = cls(value["filename"], value["url"], value["sha256"])
        if not asset.filename or Path(asset.filename).name != asset.filename:
            raise ValueError(f"{description} has an invalid filename")
        if not asset.url.startswith("https://"):
            raise ValueError(f"{description} URL must use HTTPS")
        if not re.fullmatch(r"[0-9a-f]{64}", asset.sha256):
            raise ValueError(f"{description} has an invalid SHA-256")
        return asset


@dataclass(frozen=True)
class LinuxInputs:
    python_version: str
    uv_version: str
    rust_target: str
    minimum_glibc: tuple[int, int]
    python_archive: LockedAsset
    uv_archive: LockedAsset
    requirements_lock: Path


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


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_sha256(path: Path, expected: str) -> None:
    actual = file_sha256(path)
    if actual != expected:
        raise ValueError(
            f"SHA-256 mismatch for {path}: expected {expected}, got {actual}"
        )


def normalized_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def parse_requirements_lock(path: Path) -> dict[str, tuple[str, str]]:
    locked: dict[str, tuple[str, str]] = {}
    for line_number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        match = LOCK_LINE.fullmatch(line)
        if match is None:
            raise ValueError(f"invalid locked requirement at {path}:{line_number}")
        name = normalized_name(match.group("name"))
        if name in locked:
            raise ValueError(f"duplicate locked requirement {name!r} in {path}")
        locked[name] = (match.group("version"), match.group("sha256"))
    actual_versions = {name: version for name, (version, _) in locked.items()}
    if actual_versions != APPROVED_REQUIREMENTS:
        raise ValueError(
            "Linux requirements lock does not match the approved complete runtime closure"
        )
    return locked


def load_inputs(repository: Path) -> LinuxInputs:
    path = repository / "build/runtime-inputs.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schemaVersion") != 1:
        raise ValueError("unsupported runtime input schema")
    python = document.get("python")
    if not isinstance(python, dict):
        raise ValueError("runtime inputs are missing the Python lock")
    if python.get("implementation") != "CPython" or python.get("abi") != "standard-gil":
        raise ValueError("Linux payload requires CPython standard-GIL")
    if python.get("provider") != "python-build-standalone":
        raise ValueError("Linux payload requires python-build-standalone")
    version = python.get("version")
    if not isinstance(version, str) or not re.fullmatch(r"3\.14\.\d+", version):
        raise ValueError("Linux payload requires a locked CPython 3.14 patch release")
    uv = document.get("uv")
    if not isinstance(uv, dict) or not isinstance(uv.get("version"), str):
        raise ValueError("runtime inputs are missing the uv version lock")
    platform = document.get("platforms", {}).get(PLATFORM)
    if not isinstance(platform, dict):
        raise ValueError(f"runtime inputs are missing {PLATFORM}")
    if platform.get("rustTarget") != "x86_64-unknown-linux-gnu":
        raise ValueError("Linux payload requires the x86_64 GNU Rust target")
    glibc_text = platform.get("minimumGlibc")
    match = re.fullmatch(r"(\d+)\.(\d+)", str(glibc_text))
    if match is None:
        raise ValueError("Linux minimum glibc must be a major.minor version")
    requirements_lock = repository / str(platform.get("requirementsLock", ""))
    if not requirements_lock.is_file():
        raise ValueError(f"Linux requirements lock is missing: {requirements_lock}")
    parse_requirements_lock(requirements_lock)
    python_archive = LockedAsset.from_json(
        platform.get("pythonArchive"), "Linux Python archive"
    )
    release = python.get("release")
    if not isinstance(release, str) or f"{version}+{release}" not in python_archive.filename:
        raise ValueError("Linux Python archive does not match the locked version and PBS release")
    return LinuxInputs(
        python_version=version,
        uv_version=uv["version"],
        rust_target=str(platform.get("rustTarget")),
        minimum_glibc=(int(match.group(1)), int(match.group(2))),
        python_archive=python_archive,
        uv_archive=LockedAsset.from_json(platform.get("uvArchive"), "Linux uv archive"),
        requirements_lock=requirements_lock,
    )


def download_locked_asset(asset: LockedAsset, cache: Path, offline: bool) -> Path:
    cache.mkdir(parents=True, exist_ok=True)
    destination = cache / asset.filename
    if destination.exists():
        verify_sha256(destination, asset.sha256)
        return destination
    if offline:
        raise ValueError(f"offline build is missing locked input {destination}")
    partial = cache / f".{asset.filename}.{os.getpid()}.partial"
    request = urllib.request.Request(asset.url, headers={"User-Agent": "kat-build/1"})
    last_error: OSError | ValueError | None = None
    for _ in range(3):
        partial.unlink(missing_ok=True)
        try:
            with urllib.request.urlopen(request, timeout=300) as response, partial.open(
                "wb"
            ) as output:
                shutil.copyfileobj(response, output, length=1024 * 1024)
            verify_sha256(partial, asset.sha256)
            partial.replace(destination)
            break
        except (OSError, ValueError) as error:
            last_error = error
    else:
        partial.unlink(missing_ok=True)
        raise ValueError(f"failed to download locked input {asset.filename}: {last_error}")
    partial.unlink(missing_ok=True)
    return destination


def resolve_locked_asset(
    asset: LockedAsset, supplied: Path | None, cache: Path, offline: bool
) -> Path:
    if supplied is None:
        return download_locked_asset(asset, cache, offline)
    supplied = supplied.resolve()
    if not supplied.is_file():
        raise ValueError(f"locked input is missing: {supplied}")
    if supplied.name != asset.filename:
        raise ValueError(
            f"locked input filename must be {asset.filename!r}, got {supplied.name!r}"
        )
    verify_sha256(supplied, asset.sha256)
    return supplied


def require_builder_python() -> None:
    if sys.version_info < (3, 12):
        raise ValueError("Linux Builder requires Python 3.12 or newer for safe tar extraction")


def safe_extract_tar(archive_path: Path, destination: Path) -> None:
    require_builder_python()
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive_path, "r:*") as archive:
        archive.extractall(destination, filter="data")


def find_uv(extracted: Path) -> Path:
    candidates = [
        path for path in extracted.rglob("uv") if path.is_file() and path.name == "uv"
    ]
    if len(candidates) != 1:
        raise ValueError(f"uv archive must contain exactly one uv executable, got {candidates}")
    candidates[0].chmod(candidates[0].stat().st_mode | stat.S_IXUSR)
    return candidates[0]


def private_python(payload: Path) -> Path:
    return payload / "python/bin/python3"


def isolated_environment(extra: dict[str, str] | None = None) -> dict[str, str]:
    environment = dict(os.environ)
    for name in PYTHON_ENVIRONMENT_VARIABLES:
        environment.pop(name, None)
    if extra:
        environment.update(extra)
    return environment


def uv_version(uv: Path) -> str:
    result = subprocess.run(
        [str(uv), "--version"],
        check=True,
        capture_output=True,
        text=True,
        env=isolated_environment(),
    )
    match = re.fullmatch(
        r"uv ([0-9]+\.[0-9]+\.[0-9]+)(?: \([^\n]+\))?", result.stdout.strip()
    )
    if match is None:
        raise ValueError(f"unexpected uv version output: {result.stdout.strip()!r}")
    return match.group(1)


def install_locked_requirements(
    uv: Path,
    python: Path,
    inputs: LinuxInputs,
    cache: Path,
    wheelhouse: Path | None,
    offline: bool,
) -> None:
    if uv_version(uv) != inputs.uv_version:
        raise ValueError(f"Linux Builder requires uv {inputs.uv_version}")
    if offline and wheelhouse is None:
        raise ValueError("offline build requires --wheelhouse for locked runtime wheels")
    command = [
        str(uv),
        "pip",
        "install",
        "--python",
        str(python),
        "--require-hashes",
        "--only-binary",
        ":all:",
        "-r",
        str(inputs.requirements_lock),
    ]
    if wheelhouse is not None:
        wheelhouse = wheelhouse.resolve()
        if not wheelhouse.is_dir():
            raise ValueError(f"wheelhouse is missing: {wheelhouse}")
        command.extend(["--no-index", "--find-links", str(wheelhouse)])
    if offline:
        command.append("--offline")
    subprocess.run(
        command,
        check=True,
        env=isolated_environment(
            {
                "UV_CACHE_DIR": str(cache),
                "UV_NO_CONFIG": "1",
                "UV_NO_PROGRESS": "1",
            }
        ),
    )


def validated_workflow_wheel(path: Path | None) -> Path:
    if path is None:
        raise ValueError("--workflow-wheel is required")
    wheel = path.resolve(strict=True)
    if wheel.name != "kat_workflow-0.1.0-py3-none-any.whl":
        raise ValueError(f"unexpected Workflow Host wheel: {wheel.name}")
    checksum = wheel.with_name(f"{wheel.name}.sha256")
    if not checksum.is_file():
        raise ValueError(f"Workflow Host wheel checksum is missing: {checksum}")
    fields = checksum.read_text("ascii").split()
    if len(fields) != 2 or fields[1] != wheel.name:
        raise ValueError(f"invalid Workflow Host wheel checksum: {checksum}")
    verify_sha256(wheel, fields[0])
    return wheel


def install_workflow_wheel(uv: Path, python: Path, wheel: Path, cache: Path) -> None:
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(python),
            "--no-deps",
            "--no-index",
            str(wheel),
        ],
        check=True,
        env=isolated_environment(
            {
                "UV_CACHE_DIR": str(cache),
                "UV_NO_CONFIG": "1",
                "UV_NO_PROGRESS": "1",
            }
        ),
    )


def prune_private_host(python_root: Path) -> None:
    site_packages = python_root / "lib"
    # KAT 非交互运行不读取该目录；其中的大小写冲突也无法在 Windows 上落盘。
    terminfo = python_root / "share/terminfo"
    if terminfo.is_dir():
        shutil.rmtree(terminfo)
    for cache in list(python_root.rglob("__pycache__")):
        if cache.is_dir():
            shutil.rmtree(cache)
    for suffix in ("*.pyc", "*.pyo", "*.whl"):
        for path in python_root.rglob(suffix):
            path.unlink()
    for package in ("pip", "setuptools", "wheel", "pkg_resources", "_distutils_hack"):
        for path in site_packages.glob(f"python*/site-packages/{package}"):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    for pattern in ("pip-*.dist-info", "setuptools-*.dist-info", "wheel-*.dist-info"):
        for path in site_packages.glob(f"python*/site-packages/{pattern}"):
            shutil.rmtree(path)
    bin_directory = python_root / "bin"
    if bin_directory.is_dir():
        for path in bin_directory.iterdir():
            if not path.name.startswith("python"):
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()


def build_cli_binary(options: BuildOptions, inputs: LinuxInputs, target_dir: Path) -> Path:
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    subprocess.run(
        [
            options.cargo,
            "build",
            "--locked",
            "--release",
            "--target",
            inputs.rust_target,
            "--manifest-path",
            str(options.repository / "kat/platform/cli/Cargo.toml"),
        ],
        check=True,
        cwd=options.repository,
        env=environment,
    )
    binary = target_dir / inputs.rust_target / "release/kat"
    if not binary.is_file():
        raise ValueError(f"Cargo did not produce the KAT CLI: {binary}")
    return binary


def paths_overlap(left: Path, right: Path) -> bool:
    left = left.resolve()
    right = right.resolve()
    return left == right or left in right.parents or right in left.parents


def reject_output_input_overlap(options: BuildOptions) -> None:
    inputs = [("download cache", options.download_cache)]
    if options.workflow_wheel is not None:
        inputs.append(("Workflow Host wheel", options.workflow_wheel))
    if options.wheelhouse is not None:
        inputs.append(("wheelhouse", options.wheelhouse))
    if options.python_archive is not None:
        inputs.append(("Python archive", options.python_archive))
    if options.uv_archive is not None:
        inputs.append(("uv archive", options.uv_archive))
    for description, path in inputs:
        if paths_overlap(options.output, path):
            raise ValueError(
                f"Linux payload output overlaps {description}: {options.output} and {path}"
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
        too_new = sorted(version for version in parse_glibc_versions(versions) if version > maximum)
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
    python = private_python(payload)
    if not cli.is_file() or not python.is_file():
        raise ValueError("Linux payload is missing kat or python/bin/python3")
    if os.name != "nt" and not os.access(cli, os.X_OK):
        raise ValueError("Linux payload KAT CLI is not executable")
    terminfo = payload / "python/share/terminfo"
    if terminfo.exists():
        raise ValueError("Linux payload contains bundled terminfo")
    forbidden_names = {
        ".git",
        ".pytest_cache",
        "__pycache__",
        "Cargo.toml",
        "pack.toml",
        "pyproject.toml",
    }
    forbidden = [
        path
        for path in payload.rglob("*")
        if path.name in forbidden_names or path.suffix in {".whl", ".pyc", ".pyo"}
    ]
    if forbidden:
        raise ValueError(f"Linux payload contains source/build/cache artifacts: {forbidden[:5]}")


def smoke_private_host(payload: Path, inputs: LinuxInputs) -> None:
    versions = json.dumps(APPROVED_REQUIREMENTS, sort_keys=True)
    script = (
        "import importlib.metadata as m, json, sys, sysconfig; "
        f"assert '.'.join(map(str, sys.version_info[:3])) == {inputs.python_version!r}; "
        "assert not bool(sysconfig.get_config_var('Py_GIL_DISABLED')); "
        "import kat, _kat_runtime; "
        f"expected=json.loads({versions!r}); "
        "actual={name:m.version(name) for name in expected}; "
        "assert actual == expected, (actual, expected)"
    )
    subprocess.run(
        [str(private_python(payload)), "-I", "-B", "-c", script],
        check=True,
        cwd=payload,
        env=isolated_environment({"PYTHONPATH": str(payload / "must-not-be-used")}),
    )


def build_payload(options: BuildOptions) -> Path:
    require_builder_python()
    repository = options.repository.resolve()
    inputs = load_inputs(repository)
    output = options.output.resolve()
    reject_output_input_overlap(options)
    if output.exists():
        raise ValueError(f"Linux payload output already exists: {output}")
    if options.offline and options.wheelhouse is None:
        raise ValueError("offline build requires --wheelhouse")
    workflow_wheel = validated_workflow_wheel(options.workflow_wheel)
    output.parent.mkdir(parents=True, exist_ok=True)
    python_archive = resolve_locked_asset(
        inputs.python_archive,
        options.python_archive,
        options.download_cache.resolve(),
        options.offline,
    )
    uv_archive = resolve_locked_asset(
        inputs.uv_archive,
        options.uv_archive,
        options.download_cache.resolve(),
        options.offline,
    )

    with tempfile.TemporaryDirectory(prefix="kat-linux-payload-", dir=output.parent) as temporary:
        temporary_root = Path(temporary)
        extracted_python = temporary_root / "python-archive"
        extracted_uv = temporary_root / "uv-archive"
        extracted_python.mkdir()
        extracted_uv.mkdir()
        safe_extract_tar(python_archive, extracted_python)
        safe_extract_tar(uv_archive, extracted_uv)
        source_python = extracted_python / "python"
        if not source_python.is_dir():
            raise ValueError("python-build-standalone archive is missing top-level python/")
        stage = temporary_root / "payload"
        stage.mkdir()
        shutil.move(str(source_python), stage / "python")
        python = private_python(stage)
        if not python.is_file():
            raise ValueError(f"Bundled Python launcher is missing: {python}")
        uv = find_uv(extracted_uv)
        install_locked_requirements(
            uv,
            python,
            inputs,
            temporary_root / "uv-cache",
            options.wheelhouse,
            options.offline,
        )
        install_workflow_wheel(
            uv, python, workflow_wheel, temporary_root / "uv-workflow-cache"
        )
        prune_private_host(stage / "python")
        cli = build_cli_binary(options, inputs, temporary_root / "cargo-target")
        shutil.copy2(cli, stage / "kat")
        (stage / "kat").chmod((stage / "kat").stat().st_mode | stat.S_IXUSR)
        assert_payload_shape(stage)
        smoke_private_host(stage, inputs)
        verify_native_baseline(stage, options.readelf, inputs.minimum_glibc)
        stage.replace(output)
    return output


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
    workflow_wheel = args.workflow_wheel or (
        repository / "target/kat/workflow-wheel/kat_workflow-0.1.0-py3-none-any.whl"
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
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
