#!/usr/bin/env python3
"""构建完整的 Windows x86_64 KAT Platform Payload。"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from collections import deque
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Container, Iterable


PLATFORM = "windows-x86_64"
PE_X86_64 = 0x8664
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
SYSTEM_API_SET_PREFIXES = ("api-ms-win-", "ext-ms-")
APPROVED_REQUIREMENTS = {
    "click": "8.4.2",
    "cloudpickle": "3.1.2",
    "colorama": "0.4.6",
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
class VCRuntimeInput:
    provider: str
    version: str
    archive: LockedAsset
    content_root: PurePosixPath

    @classmethod
    def from_json(cls, value: Any) -> "VCRuntimeInput":
        expected = {"provider", "version", "archive", "contentRoot"}
        if not isinstance(value, dict) or set(value) != expected:
            raise ValueError("Windows VC Runtime source lock is incomplete")
        runtime = cls(
            provider=str(value["provider"]),
            version=str(value["version"]),
            archive=LockedAsset.from_json(value["archive"], "Windows VC Runtime VSIX"),
            content_root=PurePosixPath(str(value["contentRoot"])),
        )
        if runtime.provider != "Visual Studio 2022 CRT Redist VSIX":
            raise ValueError(
                "Windows VC Runtime must come from the locked Microsoft VSIX"
            )
        if not re.fullmatch(r"14\.44\.\d+", runtime.version):
            raise ValueError("Windows VC Runtime must lock a VC143 patch version")
        if (
            runtime.content_root.is_absolute()
            or any(part in {"", ".", ".."} for part in runtime.content_root.parts)
            or runtime.content_root.name != "Microsoft.VC143.CRT"
        ):
            raise ValueError("Windows VC Runtime VSIX content root is invalid")
        return runtime


@dataclass(frozen=True)
class WindowsInputs:
    python_version: str
    uv_version: str
    rust_target: str
    minimum_windows: int
    python_archive: LockedAsset
    uv_archive: LockedAsset
    vc_runtime: VCRuntimeInput
    requirements_lock: Path


@dataclass(frozen=True)
class BuildOptions:
    repository: Path
    output: Path
    download_cache: Path
    python_archive: Path | None
    uv_archive: Path | None
    wheelhouse: Path | None
    vc_redist_archive: Path | None
    cargo: str
    readobj: str
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
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), 1
    ):
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
            "Windows requirements lock does not match the approved complete runtime closure"
        )
    return locked


def load_inputs(repository: Path) -> WindowsInputs:
    path = repository / "build/runtime-inputs.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schemaVersion") != 1:
        raise ValueError("unsupported runtime input schema")
    python = document.get("python")
    if not isinstance(python, dict):
        raise ValueError("runtime inputs are missing the Python lock")
    if python.get("implementation") != "CPython" or python.get("abi") != "standard-gil":
        raise ValueError("Windows payload requires CPython standard-GIL")
    if python.get("provider") != "python-build-standalone":
        raise ValueError("Windows payload requires python-build-standalone")
    version = python.get("version")
    if not isinstance(version, str) or not re.fullmatch(r"3\.14\.\d+", version):
        raise ValueError("Windows payload requires a locked CPython 3.14 patch release")
    uv = document.get("uv")
    if not isinstance(uv, dict) or not isinstance(uv.get("version"), str):
        raise ValueError("runtime inputs are missing the uv version lock")
    platform = document.get("platforms", {}).get(PLATFORM)
    if not isinstance(platform, dict):
        raise ValueError(f"runtime inputs are missing {PLATFORM}")
    if platform.get("rustTarget") != "x86_64-pc-windows-msvc":
        raise ValueError("Windows payload requires the x86_64 MSVC Rust target")
    if platform.get("minimumWindows") != "10":
        raise ValueError("Windows payload supports Windows 10 or newer clients")
    requirements_lock = repository / str(platform.get("requirementsLock", ""))
    if not requirements_lock.is_file():
        raise ValueError(f"Windows requirements lock is missing: {requirements_lock}")
    parse_requirements_lock(requirements_lock)
    python_archive = LockedAsset.from_json(
        platform.get("pythonArchive"), "Windows Python archive"
    )
    release = python.get("release")
    if (
        not isinstance(release, str)
        or f"{version}+{release}" not in python_archive.filename
    ):
        raise ValueError(
            "Windows Python archive does not match the locked version and PBS release"
        )
    return WindowsInputs(
        python_version=version,
        uv_version=uv["version"],
        rust_target=str(platform.get("rustTarget")),
        minimum_windows=10,
        python_archive=python_archive,
        uv_archive=LockedAsset.from_json(
            platform.get("uvArchive"), "Windows uv archive"
        ),
        vc_runtime=VCRuntimeInput.from_json(platform.get("vcRuntime")),
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
            with (
                urllib.request.urlopen(request, timeout=300) as response,
                partial.open("wb") as output,
            ):
                shutil.copyfileobj(response, output, length=1024 * 1024)
            verify_sha256(partial, asset.sha256)
            partial.replace(destination)
            break
        except (OSError, ValueError) as error:
            last_error = error
    else:
        partial.unlink(missing_ok=True)
        raise ValueError(
            f"failed to download locked input {asset.filename}: {last_error}"
        )
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
        raise ValueError(
            "Windows Builder requires Python 3.12 or newer for safe tar extraction"
        )


def require_windows_builder() -> None:
    if os.name != "nt":
        raise ValueError("Windows payload must be built on native Windows")


def safe_extract_tar(archive_path: Path, destination: Path) -> None:
    require_builder_python()
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive_path, "r:*") as archive:
        archive.extractall(destination, filter="data")


def safe_extract_zip(archive_path: Path, destination: Path) -> None:
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive_path) as archive:
        seen: set[str] = set()
        for member in archive.infolist():
            name = member.filename
            path = PurePosixPath(name)
            key = "/".join(path.parts).casefold()
            mode = member.external_attr >> 16
            if (
                not name
                or "\\" in name
                or path.is_absolute()
                or any(part in {"", ".", ".."} for part in path.parts)
                or ":" in path.parts[0]
                or stat.S_ISLNK(mode)
            ):
                raise ValueError(f"unsafe zip member: {name!r}")
            if key in seen:
                raise ValueError(f"duplicate zip member: {name!r}")
            seen.add(key)
            target = (destination / Path(*path.parts)).resolve()
            try:
                target.relative_to(destination)
            except ValueError as error:
                raise ValueError(f"zip member escapes destination: {name!r}") from error
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=False)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(member) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)


def find_uv(extracted: Path) -> Path:
    candidates = [
        path
        for path in extracted.rglob("*")
        if path.is_file() and path.name.casefold() == "uv.exe"
    ]
    if len(candidates) != 1:
        raise ValueError(
            f"uv archive must contain exactly one uv.exe, got {candidates}"
        )
    return candidates[0]


def private_python(payload: Path) -> Path:
    return payload / "python/python.exe"


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
    inputs: WindowsInputs,
    cache: Path,
    wheelhouse: Path | None,
    offline: bool,
) -> None:
    if uv_version(uv) != inputs.uv_version:
        raise ValueError(f"Windows Builder requires uv {inputs.uv_version}")
    if offline and wheelhouse is None:
        raise ValueError(
            "offline build requires --wheelhouse for locked runtime wheels"
        )
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
                "UV_LINK_MODE": "copy",
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
                "UV_LINK_MODE": "copy",
                "UV_NO_CONFIG": "1",
                "UV_NO_PROGRESS": "1",
            }
        ),
    )


def prune_private_host(python_root: Path) -> None:
    for cache in list(python_root.rglob("__pycache__")):
        if cache.is_dir():
            shutil.rmtree(cache)
    for suffix in ("*.pyc", "*.pyo", "*.whl"):
        for path in python_root.rglob(suffix):
            path.unlink()
    site_packages = python_root / "Lib/site-packages"
    for package in ("pip", "setuptools", "wheel", "pkg_resources", "_distutils_hack"):
        for path in site_packages.glob(package):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    for pattern in ("pip-*.dist-info", "setuptools-*.dist-info", "wheel-*.dist-info"):
        for path in site_packages.glob(pattern):
            shutil.rmtree(path)
    scripts = python_root / "Scripts"
    if scripts.is_dir():
        shutil.rmtree(scripts)


def build_cli_binary(
    options: BuildOptions, inputs: WindowsInputs, target_dir: Path
) -> Path:
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    environment["RUSTFLAGS"] = "-C target-feature=+crt-static"
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
    binary = target_dir / inputs.rust_target / "release/kat.exe"
    if not binary.is_file():
        raise ValueError(f"Cargo did not produce the KAT CLI: {binary}")
    return binary


def pe_machine(path: Path) -> int | None:
    with path.open("rb") as source:
        if source.read(2) != b"MZ":
            return None
        source.seek(0x3C)
        offset_bytes = source.read(4)
        if len(offset_bytes) != 4:
            raise ValueError(f"truncated DOS header in PE file: {path}")
        pe_offset = struct.unpack("<I", offset_bytes)[0]
        source.seek(pe_offset)
        if source.read(4) != b"PE\0\0":
            raise ValueError(f"invalid PE signature: {path}")
        machine_bytes = source.read(2)
        if len(machine_bytes) != 2:
            raise ValueError(f"truncated PE header: {path}")
        return struct.unpack("<H", machine_bytes)[0]


def pe_files(root: Path) -> list[Path]:
    files = (
        [root]
        if root.is_file()
        else sorted(path for path in root.rglob("*") if path.is_file())
    )
    result: list[Path] = []
    for path in files:
        machine = pe_machine(path)
        if machine is None:
            continue
        if machine != PE_X86_64:
            raise ValueError(f"Windows payload contains a non-x86_64 PE file: {path}")
        result.append(path.resolve())
    return result


def index_pe_paths(paths: Iterable[Path], description: str) -> dict[str, Path]:
    index: dict[str, Path] = {}
    hashes: dict[str, str] = {}
    for path in paths:
        machine = pe_machine(path)
        if machine is None:
            continue
        if machine != PE_X86_64:
            raise ValueError(f"{description} contains a non-x86_64 PE file: {path}")
        name = path.name.casefold()
        digest = file_sha256(path)
        if name in index and hashes[name] != digest:
            raise ValueError(
                f"{description} has ambiguous PE basename {path.name!r}: "
                f"{index[name]} and {path}"
            )
        index.setdefault(name, path.resolve())
        hashes.setdefault(name, digest)
    return index


def index_pe_tree(root: Path, description: str) -> dict[str, Path]:
    return index_pe_paths(pe_files(root), description)


def index_vc_redist(root: Path) -> dict[str, Path]:
    candidates = sorted(
        path
        for path in root.rglob("*")
        if path.is_file() and path.suffix.casefold() == ".dll"
    )
    index = index_pe_paths(candidates, "VC Runtime redistributable source")
    if not index:
        raise ValueError(
            f"VC Runtime redistributable source has no x86_64 DLLs: {root}"
        )
    return index


def parse_readobj_imports(output: str, path: Path) -> set[str]:
    imports: set[str] = set()
    block_depth = 0
    expects_name = False
    for raw_line in output.splitlines():
        line = raw_line.strip()
        if line in {"Import {", "DelayImport {"}:
            block_depth += 1
            if block_depth == 1:
                expects_name = True
            continue
        if line == "}":
            if block_depth == 1 and expects_name:
                raise ValueError(f"llvm-readobj import block has no DLL name in {path}")
            block_depth = max(0, block_depth - 1)
            continue
        if block_depth == 1 and expects_name and line.startswith("Name:"):
            raw_name = line.removeprefix("Name:").strip()
            name = PureWindowsPath(raw_name).name
            if name != raw_name or not name.casefold().endswith(".dll"):
                raise ValueError(f"invalid PE import {raw_name!r} in {path}")
            imports.add(name.casefold())
            expects_name = False
    if block_depth:
        raise ValueError(f"unterminated llvm-readobj import block in {path}")
    return imports


def pe_imports(path: Path, readobj: str) -> set[str]:
    result = subprocess.run(
        [readobj, "--coff-imports", str(path)],
        check=True,
        capture_output=True,
        text=True,
        env=isolated_environment({"LC_ALL": "C"}),
    )
    return parse_readobj_imports(result.stdout, path)


def windows_file_version_values(path: Path, key: str) -> set[str]:
    if os.name != "nt":
        raise ValueError("Windows file version metadata requires native Windows")
    from ctypes import wintypes

    version = ctypes.WinDLL("version", use_last_error=True)
    version.GetFileVersionInfoSizeW.argtypes = [wintypes.LPCWSTR, wintypes.LPDWORD]
    version.GetFileVersionInfoSizeW.restype = wintypes.DWORD
    version.GetFileVersionInfoW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.LPVOID,
    ]
    version.GetFileVersionInfoW.restype = wintypes.BOOL
    version.VerQueryValueW.argtypes = [
        wintypes.LPCVOID,
        wintypes.LPCWSTR,
        ctypes.POINTER(wintypes.LPVOID),
        wintypes.PUINT,
    ]
    version.VerQueryValueW.restype = wintypes.BOOL

    ignored = wintypes.DWORD()
    size = version.GetFileVersionInfoSizeW(str(path), ctypes.byref(ignored))
    if not size:
        return set()
    buffer = ctypes.create_string_buffer(size)
    if not version.GetFileVersionInfoW(str(path), 0, size, buffer):
        return set()
    translations_pointer = wintypes.LPVOID()
    translations_size = wintypes.UINT()
    if not version.VerQueryValueW(
        buffer,
        r"\VarFileInfo\Translation",
        ctypes.byref(translations_pointer),
        ctypes.byref(translations_size),
    ):
        return set()
    translations = ctypes.string_at(translations_pointer, translations_size.value)
    if len(translations) % 4:
        raise ValueError(f"invalid version translation table in {path}")
    values: set[str] = set()
    for offset in range(0, len(translations), 4):
        language, code_page = struct.unpack_from("<HH", translations, offset)
        value_pointer = wintypes.LPVOID()
        value_size = wintypes.UINT()
        query = rf"\StringFileInfo\{language:04x}{code_page:04x}\{key}"
        if version.VerQueryValueW(
            buffer,
            query,
            ctypes.byref(value_pointer),
            ctypes.byref(value_size),
        ):
            value = ctypes.wstring_at(value_pointer, value_size.value).rstrip("\0")
            if value:
                values.add(value)
    return values


def is_windows_system_component(path: Path) -> bool:
    companies = windows_file_version_values(path, "CompanyName")
    products = windows_file_version_values(path, "ProductName")
    return any("microsoft" in value.casefold() for value in companies) and any(
        "windows" in value.casefold() or value.casefold() == "internet explorer"
        for value in products
    )


class WindowsSystemDllNames:
    def __init__(self, root: Path) -> None:
        self.root = root
        self._cache: dict[str, bool] = {}

    def __contains__(self, name: object) -> bool:
        if not isinstance(name, str):
            return False
        normalized = name.casefold()
        if normalized not in self._cache:
            path = self.root / normalized
            self._cache[normalized] = path.is_file() and is_windows_system_component(
                path
            )
        return self._cache[normalized]


def windows_system_dll_names() -> WindowsSystemDllNames:
    system_root = os.environ.get("SystemRoot")
    if not system_root:
        raise ValueError("SystemRoot is required to identify Windows system DLLs")
    system32 = Path(system_root) / "System32"
    if not system32.is_dir():
        raise ValueError(f"Windows system directory is missing: {system32}")
    return WindowsSystemDllNames(system32)


def is_system_import(name: str, system_names: Container[str]) -> bool:
    return name.startswith(SYSTEM_API_SET_PREFIXES) or name in system_names


def payload_search_directories(path: Path, application_directory: Path) -> set[Path]:
    directories = {path.parent, application_directory}
    site_packages = application_directory / "Lib" / "site-packages"
    try:
        relative = path.relative_to(site_packages)
    except ValueError:
        return directories
    if len(relative.parts) < 2:
        return directories

    package = relative.parts[0]
    package_directory = site_packages / package
    if package_directory.is_dir():
        # delvewheel 通过 AddDllDirectory 注册这个由 package 拥有的目录。
        directories.add(site_packages / f"{package}.libs")
    return directories


def collect_vc_runtime_closure(
    roots: Iterable[Path],
    payload_index: dict[str, Path],
    application_directory: Path,
    redist_index: dict[str, Path],
    system_names: Container[str],
    readobj: str,
    *,
    allow_redist: bool,
) -> set[Path]:
    application_directory = application_directory.resolve()
    queue = deque(path.resolve() for path in roots)
    visited: set[Path] = set()
    required: dict[str, Path] = {}
    while queue:
        path = queue.popleft()
        if path in visited:
            continue
        visited.add(path)
        machine = pe_machine(path)
        if machine != PE_X86_64:
            raise ValueError(
                f"native dependency closure contains unsupported PE: {path}"
            )
        for name in sorted(pe_imports(path, readobj)):
            bundled = payload_index.get(name)
            if bundled is not None and bundled.parent in payload_search_directories(
                path, application_directory
            ):
                queue.append(bundled)
                continue
            redistributable = redist_index.get(name)
            if redistributable is not None:
                if not allow_redist:
                    raise ValueError(
                        f"app-local VC Runtime dependency {name!r} is missing for {path}"
                    )
                required[name] = redistributable
                queue.append(redistributable)
                continue
            if is_system_import(name, system_names):
                continue
            raise ValueError(f"unresolved PE dependency {name!r} imported by {path}")
    return set(required.values())


def remove_noncanonical_vc_runtime(payload: Path, redist_names: set[str]) -> None:
    for path in sorted(
        candidate for candidate in payload.rglob("*") if candidate.is_file()
    ):
        if path.name.casefold() in redist_names:
            path.unlink()


def copy_vc_runtime_closure(required: Iterable[Path], destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    for source in sorted(required, key=lambda path: path.name.casefold()):
        target = destination / source.name
        if target.exists():
            if file_sha256(target) != file_sha256(source):
                raise ValueError(
                    f"conflicting app-local VC Runtime destination: {target}"
                )
            continue
        shutil.copy2(source, target)


def collect_and_copy_vc_runtime(payload: Path, redist_root: Path, readobj: str) -> None:
    redist_index = index_vc_redist(redist_root)
    remove_noncanonical_vc_runtime(payload, set(redist_index))
    system_names = windows_system_dll_names()

    cli = payload / "kat.exe"
    cli_index = index_pe_paths([cli], "KAT CLI process image")
    cli_runtime = collect_vc_runtime_closure(
        [cli],
        cli_index,
        payload,
        redist_index,
        system_names,
        readobj,
        allow_redist=True,
    )
    if cli_runtime:
        raise ValueError(
            "KAT CLI imports the dynamic VC Runtime; the MSVC build must statically link its CRT"
        )

    python_root = payload / "python"
    python_index = index_pe_tree(python_root, "Bundled Python Host")
    python_runtime = collect_vc_runtime_closure(
        python_index.values(),
        python_index,
        python_root,
        redist_index,
        system_names,
        readobj,
        allow_redist=True,
    )

    copy_vc_runtime_closure(python_runtime, python_root)

    final_cli_index = index_pe_paths(
        [path for path in payload.iterdir() if path.is_file()], "KAT CLI process image"
    )
    collect_vc_runtime_closure(
        [cli],
        final_cli_index,
        payload,
        redist_index,
        system_names,
        readobj,
        allow_redist=False,
    )
    final_python_index = index_pe_tree(python_root, "Bundled Python Host")
    collect_vc_runtime_closure(
        final_python_index.values(),
        final_python_index,
        python_root,
        redist_index,
        system_names,
        readobj,
        allow_redist=False,
    )


def resolve_vc_redist_root(extracted: Path, locked: VCRuntimeInput) -> Path:
    root = (extracted / Path(*locked.content_root.parts)).resolve()
    if not root.is_dir():
        raise ValueError(f"VC Runtime redistributable source is missing: {root}")
    try:
        root.relative_to(extracted.resolve())
    except ValueError as error:
        raise ValueError(
            f"VC Runtime VSIX content root escapes extraction: {root}"
        ) from error
    if any(part.casefold() == "debug_nonredist" for part in root.parts):
        raise ValueError("debug_nonredist files are not redistributable")
    return root


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
    if options.vc_redist_archive is not None:
        inputs.append(("VC Runtime VSIX", options.vc_redist_archive))
    for description, path in inputs:
        if paths_overlap(options.output, path):
            raise ValueError(
                f"Windows payload output overlaps {description}: {options.output} and {path}"
            )


def assert_payload_shape(payload: Path) -> None:
    cli = payload / "kat.exe"
    python = private_python(payload)
    if not cli.is_file() or not python.is_file():
        raise ValueError("Windows payload is missing kat.exe or python/python.exe")
    root_entries = {path.name for path in payload.iterdir()}
    if root_entries != {"kat.exe", "python"}:
        raise ValueError(
            "Windows payload root must contain only kat.exe and the private python/ directory, "
            f"got {root_entries}"
        )
    root_executables = {
        path.name.casefold()
        for path in payload.iterdir()
        if path.is_file() and path.suffix.casefold() == ".exe"
    }
    if root_executables != {"kat.exe"}:
        raise ValueError(
            f"Windows payload root must expose only kat.exe, got {root_executables}"
        )
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
        if path.name in forbidden_names
        or path.suffix.casefold() in {".whl", ".pyc", ".pyo", ".msi"}
        or path.name.casefold().startswith("vc_redist")
    ]
    if forbidden:
        raise ValueError(
            f"Windows payload contains source/build/cache artifacts: {forbidden[:5]}"
        )


def smoke_private_host(payload: Path, inputs: WindowsInputs) -> None:
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
    environment = isolated_environment(
        {"PATH": "", "PYTHONPATH": str(payload / "must-not-be-used")}
    )
    subprocess.run(
        [str(private_python(payload)), "-I", "-B", "-c", script],
        check=True,
        cwd=payload,
        env=environment,
    )
    subprocess.run(
        [str(payload / "kat.exe"), "--help"],
        check=True,
        capture_output=True,
        cwd=payload,
        env=environment,
    )


def build_payload(options: BuildOptions) -> Path:
    require_builder_python()
    require_windows_builder()
    repository = options.repository.resolve()
    inputs = load_inputs(repository)
    output = options.output.resolve()
    reject_output_input_overlap(options)
    if output.exists():
        raise ValueError(f"Windows payload output already exists: {output}")
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
    vc_redist_archive = resolve_locked_asset(
        inputs.vc_runtime.archive,
        options.vc_redist_archive,
        options.download_cache.resolve(),
        options.offline,
    )

    with tempfile.TemporaryDirectory(
        prefix="kat-windows-payload-", dir=output.parent
    ) as temporary:
        temporary_root = Path(temporary)
        extracted_python = temporary_root / "python-archive"
        extracted_uv = temporary_root / "uv-archive"
        extracted_redist = temporary_root / "vc-redist-vsix"
        extracted_python.mkdir()
        extracted_uv.mkdir()
        extracted_redist.mkdir()
        safe_extract_tar(python_archive, extracted_python)
        safe_extract_zip(uv_archive, extracted_uv)
        safe_extract_zip(vc_redist_archive, extracted_redist)
        redist_root = resolve_vc_redist_root(extracted_redist, inputs.vc_runtime)
        source_python = extracted_python / "python"
        if not source_python.is_dir():
            raise ValueError(
                "python-build-standalone archive is missing top-level python/"
            )
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
        shutil.copy2(cli, stage / "kat.exe")
        collect_and_copy_vc_runtime(stage, redist_root, options.readobj)
        assert_payload_shape(stage)
        smoke_private_host(stage, inputs)
        stage.replace(output)
    return output


def parse_args(argv: list[str] | None = None) -> BuildOptions:
    repository_default = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Build the complete Windows x86_64 KAT Platform Payload"
    )
    parser.add_argument("--repository", type=Path, default=repository_default)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--download-cache", type=Path)
    parser.add_argument("--python-archive", type=Path)
    parser.add_argument("--uv-archive", type=Path)
    parser.add_argument("--wheelhouse", type=Path)
    parser.add_argument("--workflow-wheel", type=Path)
    parser.add_argument(
        "--vc-redist-archive",
        type=Path,
        help="pre-fetched locked Microsoft CRT Redist VSIX",
    )
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument(
        "--readobj",
        default="llvm-readobj",
        help="LLVM PE inspector; --coff-imports includes regular and delay imports",
    )
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args(argv)
    repository = args.repository.resolve()
    output = args.output or repository / "target/kat/payloads/windows-x86_64"
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
        vc_redist_archive=args.vc_redist_archive,
        cargo=args.cargo,
        readobj=args.readobj,
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
