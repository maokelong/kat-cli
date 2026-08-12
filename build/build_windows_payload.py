#!/usr/bin/env python3
"""构建完整的 Windows x86_64 KAT Platform Payload。"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tarfile
import zipfile
from collections import deque
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Container, Iterable

import pefile
import payload_builder


PLATFORM = "windows-x86_64"
PLATFORM_SPEC = payload_builder.PlatformSpec(
    key=PLATFORM,
    label="Windows",
    managed_python_fields=("windows-x86_64-none", "windows", "none"),
    managed_python_launcher_glob="*/python.exe",
    managed_python_root_parents=1,
    private_python_parts=("python", "python.exe"),
    copy_uv_links=True,
    site_packages_globs=("Lib/site-packages",),
    prune_paths=(("Scripts",),),
    private_bin_parts=None,
    private_bin_keep_prefix=None,
    cli_filename="kat.exe",
    cargo_environment=(
        ("RUSTFLAGS", "-C target-feature=+crt-static"),
        ("LINK", "/Brepro"),
    ),
    forbidden_payload_suffixes=frozenset({".msi"}),
    forbidden_payload_prefixes=("vc_redist",),
)
PE_X86_64 = 0x8664
SYSTEM_API_SET_PREFIXES = ("api-ms-win-", "ext-ms-")
@dataclass(frozen=True)
class VCRuntimeInput:
    provider: str
    archive: payload_builder.LockedAsset
    content_root: PurePosixPath

    @classmethod
    def from_json(cls, value: Any) -> "VCRuntimeInput":
        expected = {"provider", "archive", "contentRoot"}
        if not isinstance(value, dict) or set(value) != expected:
            raise ValueError("Windows VC Runtime source lock is incomplete")
        runtime = cls(
            provider=str(value["provider"]),
            archive=payload_builder.LockedAsset.from_json(
                value["archive"], "Windows VC Runtime VSIX"
            ),
            content_root=PurePosixPath(str(value["contentRoot"])),
        )
        if runtime.provider != "Visual Studio 2022 CRT Redist VSIX":
            raise ValueError(
                "Windows VC Runtime must come from the locked Microsoft VSIX"
            )
        if (
            runtime.content_root.is_absolute()
            or any(part in {"", ".", ".."} for part in runtime.content_root.parts)
            or runtime.content_root.name != "Microsoft.VC143.CRT"
        ):
            raise ValueError("Windows VC Runtime VSIX content root is invalid")
        return runtime


@dataclass(frozen=True)
class WindowsInputs(payload_builder.CommonInputs):
    minimum_windows: int
    vc_runtime: VCRuntimeInput


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
    offline: bool
    workflow_wheel: Path | None = None


def require_windows_builder() -> None:
    if os.name != "nt":
        raise ValueError("Windows payload must be built on native Windows")


def load_inputs(repository: Path) -> WindowsInputs:
    common, platform = payload_builder.load_common_inputs(
        repository,
        platform_name=PLATFORM,
        rust_target="x86_64-pc-windows-msvc",
        platform_label="Windows",
    )
    if platform.get("minimumWindows") != "10":
        raise ValueError("Windows payload supports Windows 10 or newer clients")
    return WindowsInputs(
        python_version=common.python_version,
        rust_target=common.rust_target,
        python_archive=common.python_archive,
        uv=common.uv,
        requirements_lock=common.requirements_lock,
        minimum_windows=10,
        vc_runtime=VCRuntimeInput.from_json(platform.get("vcRuntime")),
    )


def pe_machine(path: Path) -> int | None:
    try:
        image = pefile.PE(str(path), fast_load=True)
    except pefile.PEFormatError:
        return None
    try:
        return int(image.FILE_HEADER.Machine)
    finally:
        image.close()


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
        digest = payload_builder.file_sha256(path)
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


def pe_imports(path: Path) -> set[str]:
    image = pefile.PE(str(path), fast_load=True)
    try:
        image.parse_data_directories(
            directories=[
                pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
                pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT"],
            ],
            import_dllnames_only=True,
        )
        imports: set[str] = set()
        for attribute in ("DIRECTORY_ENTRY_IMPORT", "DIRECTORY_ENTRY_DELAY_IMPORT"):
            for entry in getattr(image, attribute, ()):
                raw_name = entry.dll.decode("ascii")
                name = PureWindowsPath(raw_name).name
                if name != raw_name or not name.casefold().endswith(".dll"):
                    raise ValueError(f"invalid PE import {raw_name!r} in {path}")
                imports.add(name.casefold())
        return imports
    finally:
        image.close()


def windows_file_version_values(path: Path, key: str) -> set[str]:
    image = pefile.PE(str(path), fast_load=False)
    try:
        values: set[str] = set()
        expected = key.encode("ascii")
        for group in getattr(image, "FileInfo", ()):
            for entry in group:
                if getattr(entry, "Key", b"") != b"StringFileInfo":
                    continue
                for table in entry.StringTable:
                    value = table.entries.get(expected)
                    if value:
                        values.add(value.decode("utf-8", errors="replace"))
        return values
    finally:
        image.close()


def is_windows_system_component(path: Path) -> bool:
    companies = windows_file_version_values(path, "CompanyName")
    products = windows_file_version_values(path, "ProductName")
    return any("microsoft" in value.casefold() for value in companies) and any(
        "windows" in value.casefold() or value.casefold() == "internet explorer"
        for value in products
    )


class BuildHostWindowsSystemDllNames:
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


def build_host_windows_system_dll_names() -> BuildHostWindowsSystemDllNames:
    """Identify OS-owned imports on the native Builder, not a client baseline."""
    system_root = os.environ.get("SystemRoot")
    if not system_root:
        raise ValueError("SystemRoot is required to inspect Builder system DLLs")
    system32 = Path(system_root) / "System32"
    if not system32.is_dir():
        raise ValueError(f"Windows Builder system directory is missing: {system32}")
    return BuildHostWindowsSystemDllNames(system32)


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
        for name in sorted(pe_imports(path)):
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
            if payload_builder.file_sha256(target) != payload_builder.file_sha256(source):
                raise ValueError(
                    f"conflicting app-local VC Runtime destination: {target}"
                )
            continue
        shutil.copy2(source, target)


def collect_and_copy_vc_runtime(payload: Path, redist_root: Path) -> None:
    redist_index = index_vc_redist(redist_root)
    remove_noncanonical_vc_runtime(payload, set(redist_index))
    # This excludes OS-owned imports from the app-local VC Runtime closure. It
    # does not establish the Windows 10 client compatibility baseline, which is
    # verified by the clean-client acceptance slice tracked in Issue #143.
    system_names = build_host_windows_system_dll_names()

    cli = payload / "kat.exe"
    cli_index = index_pe_paths([cli], "KAT CLI process image")
    cli_runtime = collect_vc_runtime_closure(
        [cli],
        cli_index,
        payload,
        redist_index,
        system_names,
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
        allow_redist=False,
    )
    final_python_index = index_pe_tree(python_root, "Bundled Python Host")
    collect_vc_runtime_closure(
        final_python_index.values(),
        final_python_index,
        python_root,
        redist_index,
        system_names,
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


def assert_payload_shape(payload: Path) -> None:
    cli = payload / "kat.exe"
    python = payload_builder.private_python(payload, PLATFORM_SPEC)
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
    payload_builder.assert_no_build_artifacts(payload, PLATFORM_SPEC)


class WindowsAdapter:
    spec = PLATFORM_SPEC

    def __init__(self, *, vc_redist_archive: Path | None) -> None:
        self.vc_redist_archive = vc_redist_archive

    def require_builder(self) -> None:
        payload_builder.require_builder_python(self.spec.label)
        require_windows_builder()

    def load_inputs(self, repository: Path) -> WindowsInputs:
        return load_inputs(repository)

    def extra_input_paths(self) -> Iterable[tuple[str, Path | None]]:
        return (("VC Runtime VSIX", self.vc_redist_archive),)

    def resolve_extra_inputs(
        self,
        inputs: WindowsInputs,
        cache: Path,
        offline: bool,
    ) -> Path:
        return payload_builder.resolve_locked_asset(
            inputs.vc_runtime.archive,
            self.vc_redist_archive,
            cache,
            offline,
        )

    def finalize_payload(
        self,
        payload: Path,
        temporary_root: Path,
        inputs: WindowsInputs,
        extra_inputs: Path,
    ) -> None:
        extracted_redist = temporary_root / "vc-redist-vsix"
        payload_builder.safe_extract_zip(extra_inputs, extracted_redist)
        redist_root = resolve_vc_redist_root(extracted_redist, inputs.vc_runtime)
        collect_and_copy_vc_runtime(payload, redist_root)

    def assert_payload_shape(self, payload: Path) -> None:
        assert_payload_shape(payload)


def build_payload(options: BuildOptions) -> Path:
    return payload_builder.build_payload(
        options,
        WindowsAdapter(vc_redist_archive=options.vc_redist_archive),
    )


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
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args(argv)
    repository = args.repository.resolve()
    output = args.output or repository / "target/kat/payloads/windows-x86_64"
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
        vc_redist_archive=args.vc_redist_archive,
        cargo=args.cargo,
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
