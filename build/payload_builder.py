"""双平台 KAT Payload 的共同构建流水线。"""

from __future__ import annotations

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
import zipfile
from dataclasses import dataclass
from email import policy
from email.parser import BytesParser
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Literal, Mapping, Protocol, TypeVar

PYTHON_ENVIRONMENT_VARIABLES = (
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONUSERBASE",
    "PYTHONINSPECT",
    "PYTHONSTARTUP",
    "PYTHONWARNINGS",
    "VIRTUAL_ENV",
)
FORBIDDEN_PAYLOAD_NAMES = {
    ".git",
    ".pytest_cache",
    "__pycache__",
    "Cargo.toml",
    "pack.toml",
    "pyproject.toml",
}
FORBIDDEN_PAYLOAD_SUFFIXES = {".whl", ".pyc", ".pyo"}
WORKFLOW_WHEEL_PATTERN = "kat_workflow-*.whl"
WORKFLOW_WHEEL_NAME = re.compile(
    r"kat_workflow-(?P<version>[^-]+)-py3-none-any\.whl"
)


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
class UvLayout:
    archive_format: Literal["tar", "zip"]
    executable: str
    needs_executable_bit: bool

    @classmethod
    def from_json(cls, value: Any, description: str) -> "UvLayout":
        expected = {"archiveFormat", "executable", "needsExecutableBit"}
        if not isinstance(value, dict) or set(value) != expected:
            raise ValueError(f"{description} layout is incomplete")
        archive_format = value["archiveFormat"]
        executable = value["executable"]
        needs_executable_bit = value["needsExecutableBit"]
        if archive_format not in {"tar", "zip"}:
            raise ValueError(f"{description} archive format must be tar or zip")
        if not isinstance(executable, str) or Path(executable).name != executable:
            raise ValueError(f"{description} executable name is invalid")
        if not isinstance(needs_executable_bit, bool):
            raise ValueError(f"{description} executable-bit flag is invalid")
        return cls(archive_format, executable, needs_executable_bit)


@dataclass(frozen=True)
class UvInput:
    version: str
    archive: LockedAsset
    layout: UvLayout


@dataclass(frozen=True)
class CommonInputs:
    python_version: str
    rust_target: str
    python_archive: LockedAsset
    uv: UvInput
    requirements_lock: Path


@dataclass(frozen=True)
class PlatformSpec:
    key: str
    label: str
    managed_python_fields: tuple[str, str, str]
    managed_python_launcher_glob: str
    managed_python_root_parents: int
    private_python_parts: tuple[str, ...]
    copy_uv_links: bool
    site_packages_globs: tuple[str, ...]
    prune_paths: tuple[tuple[str, ...], ...]
    private_bin_parts: tuple[str, ...] | None
    private_bin_keep_prefix: str | None
    cli_filename: str
    cargo_environment: tuple[tuple[str, str], ...]
    forbidden_payload_suffixes: frozenset[str] = frozenset()
    forbidden_payload_prefixes: tuple[str, ...] = ()


class CommonBuildOptions(Protocol):
    repository: Path
    output: Path
    download_cache: Path
    python_archive: Path | None
    uv_archive: Path | None
    wheelhouse: Path | None
    cargo: str
    offline: bool
    workflow_wheel: Path | None


InputsT = TypeVar("InputsT", bound=CommonInputs)
ExtraInputsT = TypeVar("ExtraInputsT")


class PlatformAdapter(Protocol[InputsT, ExtraInputsT]):
    spec: PlatformSpec

    def require_builder(self) -> None: ...

    def load_inputs(self, repository: Path) -> InputsT: ...

    def extra_input_paths(self) -> Iterable[tuple[str, Path | None]]: ...

    def resolve_extra_inputs(
        self,
        inputs: InputsT,
        cache: Path,
        offline: bool,
    ) -> ExtraInputsT: ...

    def finalize_payload(
        self,
        payload: Path,
        temporary_root: Path,
        inputs: InputsT,
        extra_inputs: ExtraInputsT,
    ) -> None: ...

    def assert_payload_shape(self, payload: Path) -> None: ...


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


def assert_no_build_artifacts(payload: Path, spec: PlatformSpec) -> None:
    suffixes = FORBIDDEN_PAYLOAD_SUFFIXES | spec.forbidden_payload_suffixes
    forbidden = [
        path
        for path in payload.rglob("*")
        if path.name in FORBIDDEN_PAYLOAD_NAMES
        or path.suffix.casefold() in suffixes
        or any(
            path.name.casefold().startswith(prefix.casefold())
            for prefix in spec.forbidden_payload_prefixes
        )
    ]
    if forbidden:
        raise ValueError(
            f"payload contains source/build/cache artifacts: {forbidden[:5]}"
        )


def _load_runtime_inputs(
    repository: Path,
    platform_name: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    path = repository / "build/runtime-inputs.json"
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or document.get("schemaVersion") != 2:
        raise ValueError("unsupported runtime input schema")
    platform = document.get("platforms", {}).get(platform_name)
    if not isinstance(platform, dict):
        raise ValueError(f"runtime inputs are missing {platform_name}")
    return document, platform


def load_uv_input(
    repository: Path,
    platform_name: str,
    platform_label: str,
) -> UvInput:
    document, platform = _load_runtime_inputs(repository, platform_name)
    return _parse_uv_input(document, platform, platform_label)


def _parse_uv_input(
    document: dict[str, Any],
    platform: dict[str, Any],
    platform_label: str,
) -> UvInput:
    uv = document.get("uv")
    if not isinstance(uv, dict) or not isinstance(uv.get("version"), str):
        raise ValueError("runtime inputs are missing the uv version lock")
    return UvInput(
        version=uv["version"],
        archive=LockedAsset.from_json(
            platform.get("uvArchive"),
            f"{platform_label} uv archive",
        ),
        layout=UvLayout.from_json(
            platform.get("uvLayout"),
            f"{platform_label} uv archive",
        ),
    )


def load_common_inputs(
    repository: Path,
    *,
    platform_name: str,
    rust_target: str,
    platform_label: str,
) -> tuple[CommonInputs, dict[str, Any]]:
    document, platform = _load_runtime_inputs(repository, platform_name)
    python = document.get("python")
    if not isinstance(python, dict):
        raise ValueError("runtime inputs are missing the Python lock")
    if python.get("implementation") != "CPython" or python.get("abi") != "standard-gil":
        raise ValueError(f"{platform_label} payload requires CPython standard-GIL")
    if python.get("provider") != "python-build-standalone":
        raise ValueError(
            f"{platform_label} payload requires python-build-standalone"
        )
    version = python.get("version")
    if not isinstance(version, str) or not re.fullmatch(r"3\.14\.\d+", version):
        raise ValueError(
            f"{platform_label} payload requires a locked CPython 3.14 patch release"
        )
    if platform.get("rustTarget") != rust_target:
        raise ValueError(
            f"{platform_label} payload requires the {rust_target} Rust target"
        )
    requirements_lock = repository / str(platform.get("requirementsLock", ""))
    if not requirements_lock.is_file():
        raise ValueError(
            f"{platform_label} requirements lock is missing: {requirements_lock}"
        )
    python_archive = LockedAsset.from_json(
        platform.get("pythonArchive"),
        f"{platform_label} Python archive",
    )
    release = python.get("release")
    if not isinstance(release, str) or f"{version}+{release}" not in python_archive.filename:
        raise ValueError(
            f"{platform_label} Python archive does not match "
            "the locked version and PBS release"
        )
    return (
        CommonInputs(
            python_version=version,
            rust_target=rust_target,
            python_archive=python_archive,
            uv=_parse_uv_input(document, platform, platform_label),
            requirements_lock=requirements_lock,
        ),
        platform,
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
    asset: LockedAsset,
    supplied: Path | None,
    cache: Path,
    offline: bool,
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


def require_builder_python(platform_label: str) -> None:
    if sys.version_info < (3, 12):
        raise ValueError(
            f"{platform_label} Builder requires Python 3.12 or newer "
            "for safe tar extraction"
        )


def safe_extract_tar(
    archive_path: Path,
    destination: Path,
    *,
    platform_label: str,
) -> None:
    require_builder_python(platform_label)
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


def private_python(payload: Path, spec: PlatformSpec) -> Path:
    return payload.joinpath(*spec.private_python_parts)


def isolated_environment(
    extra: Mapping[str, str] | None = None,
) -> dict[str, str]:
    environment = dict(os.environ)
    for name in PYTHON_ENVIRONMENT_VARIABLES:
        environment.pop(name, None)
    if extra:
        environment.update(extra)
    return environment


def _uv_environment(cache: Path, *, copy_links: bool) -> dict[str, str]:
    extra = {
        "UV_CACHE_DIR": str(cache),
        "UV_NO_CONFIG": "1",
        "UV_NO_PROGRESS": "1",
    }
    if copy_links:
        extra["UV_LINK_MODE"] = "copy"
    return isolated_environment(extra)


def uv_version(uv: Path) -> str:
    result = subprocess.run(
        [str(uv), "--version"],
        check=True,
        capture_output=True,
        text=True,
        env=isolated_environment(),
    )
    match = re.fullmatch(
        r"uv ([0-9]+\.[0-9]+\.[0-9]+)(?: \([^\n]+\))?",
        result.stdout.strip(),
    )
    if match is None:
        raise ValueError(f"unexpected uv version output: {result.stdout.strip()!r}")
    return match.group(1)


def install_private_python(
    *,
    uv: Path,
    python_archive: Path,
    inputs: CommonInputs,
    stage: Path,
    temporary_root: Path,
    spec: PlatformSpec,
) -> None:
    platform_key, os_name, libc = spec.managed_python_fields
    installation_key = f"cpython-{inputs.python_version}-{platform_key}"
    major, minor, patch = map(int, inputs.python_version.split("."))
    downloads = {
        installation_key: {
            "name": "cpython",
            "arch": {"family": "x86_64", "variant": None},
            "os": os_name,
            "libc": libc,
            "major": major,
            "minor": minor,
            "patch": patch,
            "prerelease": "",
            "url": python_archive.as_uri(),
            "sha256": inputs.python_archive.sha256,
            "variant": None,
        }
    }
    downloads_path = temporary_root / "python-downloads.json"
    downloads_path.write_text(
        json.dumps(downloads, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )

    install_directory = temporary_root / "managed-python"
    subprocess.run(
        [
            str(uv),
            "python",
            "install",
            installation_key,
            "--install-dir",
            str(install_directory),
            "--python-downloads-json-url",
            str(downloads_path),
            "--managed-python",
            "--no-bin",
            "--no-registry",
            "--no-config",
            "--no-progress",
            "--offline",
            "--cache-dir",
            str(temporary_root / "uv-python-cache"),
        ],
        check=True,
        env=isolated_environment(),
    )

    launchers = install_directory.glob(spec.managed_python_launcher_glob)
    roots = set()
    for launcher in launchers:
        root = launcher
        for _ in range(spec.managed_python_root_parents):
            root = root.parent
        roots.add(root.resolve())
    if len(roots) != 1:
        raise ValueError(
            f"uv must install exactly one {spec.label} Python, got {sorted(roots)}"
        )
    source = roots.pop()
    try:
        source.relative_to(install_directory.resolve())
    except ValueError as error:
        raise ValueError(
            f"uv installed {spec.label} Python outside the requested directory"
        ) from error
    stage.mkdir()
    shutil.move(str(source), stage / "python")
    python = private_python(stage, spec)
    if not python.is_file():
        raise ValueError(f"Bundled Python launcher is missing: {python}")


def install_locked_requirements(
    uv: Path,
    python: Path,
    inputs: CommonInputs,
    cache: Path,
    wheelhouse: Path | None,
    offline: bool,
    *,
    copy_links: bool,
) -> None:
    if offline and wheelhouse is None:
        raise ValueError("offline build requires --wheelhouse for locked runtime wheels")
    command = [
        str(uv),
        "pip",
        "sync",
        "--python",
        str(python),
        "--require-hashes",
        "--break-system-packages",
        "--only-binary",
        ":all:",
        "--strict",
        "--no-config",
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
        env=_uv_environment(cache, copy_links=copy_links),
    )


def validate_workflow_wheel_archive(path: Path) -> str:
    match = WORKFLOW_WHEEL_NAME.fullmatch(path.name)
    if match is None or not path.is_file():
        raise ValueError(f"unexpected Workflow Host wheel: {path}")
    version = match.group("version")
    dist_info = f"kat_workflow-{version}.dist-info"
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        required = {
            "kat/__init__.py",
            "_kat_runtime/__main__.py",
            f"{dist_info}/METADATA",
            f"{dist_info}/WHEEL",
        }
        missing = sorted(required - names)
        if missing:
            raise ValueError(f"Workflow Host wheel is incomplete: {missing}")

        metadata = BytesParser(policy=policy.default).parsebytes(
            archive.read(f"{dist_info}/METADATA")
        )
        if metadata.get("Name") != "kat-workflow":
            raise ValueError("Workflow Host wheel has an unexpected distribution")
        if metadata.get("Version") != version:
            raise ValueError("Workflow Host wheel version does not match its filename")

        wheel_metadata = BytesParser(policy=policy.default).parsebytes(
            archive.read(f"{dist_info}/WHEEL")
        )
        if wheel_metadata.get("Root-Is-Purelib", "").lower() != "true":
            raise ValueError("Workflow Host wheel must be pure Python")
        if wheel_metadata.get_all("Tag", []) != ["py3-none-any"]:
            raise ValueError("Workflow Host wheel must use the py3-none-any tag")
    return version


def find_workflow_wheel(directory: Path) -> Path:
    directory = directory.resolve(strict=True)
    if not directory.is_dir():
        raise ValueError(f"Workflow Host wheel directory is not a directory: {directory}")
    wheels = sorted(directory.glob(WORKFLOW_WHEEL_PATTERN))
    if len(wheels) != 1:
        raise ValueError(f"expected one Workflow Host wheel, found {len(wheels)}")
    return wheels[0]


def validated_workflow_wheel(path: Path | None) -> Path:
    if path is None:
        raise ValueError("--workflow-wheel is required")
    wheel = path.resolve(strict=True)
    checksum = wheel.with_name(f"{wheel.name}.sha256")
    if not checksum.is_file():
        raise ValueError(f"Workflow Host wheel checksum is missing: {checksum}")
    fields = checksum.read_text("ascii").split()
    if len(fields) != 2 or fields[1] != wheel.name:
        raise ValueError(f"invalid Workflow Host wheel checksum: {checksum}")
    verify_sha256(wheel, fields[0])
    validate_workflow_wheel_archive(wheel)
    return wheel


def install_workflow_wheel(
    uv: Path,
    python: Path,
    wheel: Path,
    cache: Path,
    *,
    copy_links: bool,
) -> None:
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(python),
            "--no-deps",
            "--no-index",
            "--break-system-packages",
            str(wheel),
        ],
        check=True,
        env=_uv_environment(cache, copy_links=copy_links),
    )


def check_private_host(
    uv: Path,
    python: Path,
    cache: Path,
    *,
    copy_links: bool,
) -> None:
    subprocess.run(
        [
            str(uv),
            "pip",
            "check",
            "--python",
            str(python),
            "--no-config",
        ],
        check=True,
        env=_uv_environment(cache, copy_links=copy_links),
    )


def _remove_path(path: Path) -> None:
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    else:
        path.unlink(missing_ok=True)


def prune_private_host(python_root: Path, spec: PlatformSpec) -> None:
    for parts in spec.prune_paths:
        path = python_root.joinpath(*parts)
        if path.exists():
            _remove_path(path)
    for cache in list(python_root.rglob("__pycache__")):
        _remove_path(cache)
    for suffix in ("*.pyc", "*.pyo", "*.whl"):
        for path in python_root.rglob(suffix):
            _remove_path(path)
    site_packages = [
        directory
        for pattern in spec.site_packages_globs
        for directory in python_root.glob(pattern)
    ]
    for directory in site_packages:
        for pattern in (
            "pip",
            "setuptools",
            "wheel",
            "pkg_resources",
            "_distutils_hack",
            "pip-*.dist-info",
            "setuptools-*.dist-info",
            "wheel-*.dist-info",
        ):
            for path in directory.glob(pattern):
                _remove_path(path)
    if spec.private_bin_parts is not None:
        bin_directory = python_root.joinpath(*spec.private_bin_parts)
        if bin_directory.is_dir():
            for path in bin_directory.iterdir():
                if spec.private_bin_keep_prefix is None or not path.name.startswith(
                    spec.private_bin_keep_prefix
                ):
                    _remove_path(path)


def paths_overlap(left: Path, right: Path) -> bool:
    left = left.resolve()
    right = right.resolve()
    return left == right or left in right.parents or right in left.parents


def reject_output_input_overlap(
    output: Path,
    inputs: Iterable[tuple[str, Path | None]],
    *,
    platform_label: str,
) -> None:
    for description, path in inputs:
        if path is not None and paths_overlap(output, path):
            raise ValueError(
                f"{platform_label} payload output overlaps {description}: "
                f"{output} and {path}"
            )


def find_uv(extracted: Path, layout: UvLayout) -> Path:
    expected = layout.executable
    candidates = [
        path
        for path in extracted.rglob("*")
        if path.is_file() and path.name.casefold() == expected.casefold()
    ]
    if len(candidates) != 1:
        raise ValueError(
            f"uv archive must contain exactly one {expected}, got {candidates}"
        )
    if layout.needs_executable_bit:
        candidates[0].chmod(candidates[0].stat().st_mode | stat.S_IXUSR)
    return candidates[0]


def _prepare_private_host(
    *,
    spec: PlatformSpec,
    stage: Path,
    temporary_root: Path,
    python_archive: Path,
    uv_archive: Path,
    inputs: CommonInputs,
    workflow_wheel: Path,
    wheelhouse: Path | None,
    offline: bool,
) -> None:
    extracted_uv = temporary_root / "uv-archive"
    if inputs.uv.layout.archive_format == "tar":
        safe_extract_tar(
            uv_archive,
            extracted_uv,
            platform_label=spec.label,
        )
    else:
        safe_extract_zip(uv_archive, extracted_uv)
    uv = find_uv(extracted_uv, inputs.uv.layout)
    if uv_version(uv) != inputs.uv.version:
        raise ValueError(f"{spec.label} Builder requires uv {inputs.uv.version}")
    install_private_python(
        uv=uv,
        python_archive=python_archive,
        inputs=inputs,
        stage=stage,
        temporary_root=temporary_root,
        spec=spec,
    )
    python = private_python(stage, spec)
    copy_links = spec.copy_uv_links
    install_locked_requirements(
        uv,
        python,
        inputs,
        temporary_root / "uv-cache",
        wheelhouse,
        offline,
        copy_links=copy_links,
    )
    install_workflow_wheel(
        uv,
        python,
        workflow_wheel,
        temporary_root / "uv-workflow-cache",
        copy_links=copy_links,
    )
    check_private_host(
        uv,
        python,
        temporary_root / "uv-check-cache",
        copy_links=copy_links,
    )
    prune_private_host(stage / "python", spec)


def _build_cli_binary(
    options: CommonBuildOptions,
    inputs: CommonInputs,
    target_dir: Path,
    *,
    spec: PlatformSpec,
) -> Path:
    environment = dict(os.environ)
    environment.pop("CARGO_TARGET_DIR", None)
    environment.pop("CARGO_BUILD_TARGET_DIR", None)
    environment.update(spec.cargo_environment)
    command = [
        options.cargo,
        "build",
        "--locked",
        "--release",
    ]
    if options.offline:
        command.append("--offline")
    command.extend(
        [
            "--target-dir",
            str(target_dir),
            "--target",
            inputs.rust_target,
            "--manifest-path",
            str(options.repository / "kat/platform/cli/Cargo.toml"),
        ]
    )
    subprocess.run(
        command,
        check=True,
        cwd=options.repository,
        env=environment,
    )
    binary = target_dir / inputs.rust_target / "release" / spec.cli_filename
    if not binary.is_file():
        raise ValueError(f"Cargo did not produce the KAT CLI: {binary}")
    return binary


def build_payload(
    options: CommonBuildOptions,
    adapter: PlatformAdapter[InputsT, ExtraInputsT],
) -> Path:
    adapter.require_builder()
    repository = options.repository.resolve()
    inputs = adapter.load_inputs(repository)
    output = options.output.resolve()
    cargo_cache = repository / "target" / "kat" / "cargo" / adapter.spec.key
    common_inputs = [
        ("Cargo cache", cargo_cache),
        ("download cache", options.download_cache),
        ("Workflow Host wheel", options.workflow_wheel),
        ("wheelhouse", options.wheelhouse),
        ("Python archive", options.python_archive),
        ("uv archive", options.uv_archive),
    ]
    reject_output_input_overlap(
        output,
        [*common_inputs, *adapter.extra_input_paths()],
        platform_label=adapter.spec.label,
    )
    if output.exists():
        raise ValueError(
            f"{adapter.spec.label} payload output already exists: {output}"
        )
    if options.offline and options.wheelhouse is None:
        raise ValueError("offline build requires --wheelhouse")
    workflow_wheel = validated_workflow_wheel(options.workflow_wheel)
    output.parent.mkdir(parents=True, exist_ok=True)
    cache = options.download_cache.resolve()
    python_archive = resolve_locked_asset(
        inputs.python_archive,
        options.python_archive,
        cache,
        options.offline,
    )
    uv_archive = resolve_locked_asset(
        inputs.uv.archive,
        options.uv_archive,
        cache,
        options.offline,
    )
    extra_inputs = adapter.resolve_extra_inputs(inputs, cache, options.offline)

    prefix = f"kat-{adapter.spec.key}-payload-"
    with tempfile.TemporaryDirectory(prefix=prefix, dir=output.parent) as temporary:
        temporary_root = Path(temporary)
        stage = temporary_root / "payload"
        _prepare_private_host(
            spec=adapter.spec,
            stage=stage,
            temporary_root=temporary_root,
            python_archive=python_archive,
            uv_archive=uv_archive,
            inputs=inputs,
            workflow_wheel=workflow_wheel,
            wheelhouse=options.wheelhouse,
            offline=options.offline,
        )
        cli = _build_cli_binary(
            options,
            inputs,
            cargo_cache,
            spec=adapter.spec,
        )
        shutil.copy2(cli, stage / adapter.spec.cli_filename)
        adapter.finalize_payload(
            stage,
            temporary_root,
            inputs,
            extra_inputs,
        )
        adapter.assert_payload_shape(stage)
        stage.replace(output)
    return output
