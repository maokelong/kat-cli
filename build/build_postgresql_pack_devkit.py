#!/usr/bin/env python3
"""组装可搬运的 Windows PostgreSQL External PACK 开发包。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import build_windows_payload
import payload_builder


PUBLIC_POSTGRESQL_CAPABILITY = "kat.common.sql.postgresql"
OPENPYXL_VERSION = "3.1.5"
XLSXWRITER_VERSION = "3.2.9"
DEFUSEDXML_VERSION = "0.7.1"


class DevkitBuildError(ValueError):
    """PostgreSQL PACK 开发包输入不完整或不可信。"""


@dataclass(frozen=True)
class LockedWheel:
    filename: str
    sha256: str


@dataclass(frozen=True)
class PostgreSqlInputs:
    platform: str
    python_version: str
    psycopg_version: str
    requirements_lock: Path
    wheels: tuple[LockedWheel, ...]


@dataclass(frozen=True)
class DevkitBuildOptions:
    repository: Path
    windows_payload: Path
    skill_source: Path
    pack_source: Path
    devkit_source: Path
    inputs_lock: Path
    wheelhouse: Path
    uv_archive: Path
    vc_redist_archive: Path
    output: Path
    archive: Path | None
    windows_payload_provenance: str


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _load_inputs(path: Path) -> PostgreSqlInputs:
    path = path.resolve()
    try:
        document: Any = json.loads(path.read_text("utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DevkitBuildError(f"failed to read PostgreSQL input lock: {path}") from error
    expected_fields = {
        "schemaVersion",
        "platform",
        "pythonVersion",
        "psycopgVersion",
        "requirementsLock",
        "wheels",
    }
    if (
        not isinstance(document, dict)
        or set(document) != expected_fields
        or document.get("schemaVersion") != 1
    ):
        raise DevkitBuildError("PostgreSQL input lock has an unsupported schema")
    platform = document.get("platform")
    python_version = document.get("pythonVersion")
    psycopg_version = document.get("psycopgVersion")
    requirements_value = document.get("requirementsLock")
    raw_wheels = document.get("wheels")
    if (
        platform != build_windows_payload.PLATFORM
        or not isinstance(python_version, str)
        or not isinstance(psycopg_version, str)
        or not isinstance(requirements_value, str)
        or not isinstance(raw_wheels, list)
        or not raw_wheels
    ):
        raise DevkitBuildError("PostgreSQL input lock is incomplete")
    requirements_relative = Path(requirements_value)
    if requirements_relative.is_absolute() or any(
        part in {"", ".", ".."} for part in requirements_relative.parts
    ):
        raise DevkitBuildError("PostgreSQL requirements lock path is invalid")
    requirements_lock = (path.parent / requirements_relative).resolve()
    if not requirements_lock.is_relative_to(path.parent) or not requirements_lock.is_file():
        raise DevkitBuildError(
            f"PostgreSQL requirements lock is missing: {requirements_lock}"
        )
    wheels: list[LockedWheel] = []
    seen_names: set[str] = set()
    for value in raw_wheels:
        if (
            not isinstance(value, dict)
            or set(value) != {"filename", "sha256"}
            or not isinstance(value["filename"], str)
            or not isinstance(value["sha256"], str)
            or len(value["sha256"]) != 64
            or Path(value["filename"]).name != value["filename"]
            or value["filename"] in seen_names
        ):
            raise DevkitBuildError("PostgreSQL input lock contains an invalid wheel")
        seen_names.add(value["filename"])
        wheels.append(LockedWheel(value["filename"], value["sha256"]))
    return PostgreSqlInputs(
        platform,
        python_version,
        psycopg_version,
        requirements_lock,
        tuple(wheels),
    )


def validate_wheelhouse(inputs_path: Path, wheelhouse: Path) -> PostgreSqlInputs:
    """校验离线 wheelhouse 与锁文件完全一致。"""

    inputs = _load_inputs(inputs_path)
    if not wheelhouse.is_dir():
        raise DevkitBuildError(f"PostgreSQL wheelhouse is missing: {wheelhouse}")
    expected_names = {wheel.filename for wheel in inputs.wheels}
    actual_names = {path.name for path in wheelhouse.iterdir() if path.is_file()}
    if actual_names != expected_names:
        raise DevkitBuildError(
            "PostgreSQL wheelhouse must contain exactly the locked wheels"
        )
    for wheel in inputs.wheels:
        path = wheelhouse / wheel.filename
        if _sha256(path) != wheel.sha256:
            raise DevkitBuildError(f"wheel SHA-256 mismatch: {wheel.filename}")
    return inputs


def _directory(path: Path, label: str) -> Path:
    if not path.is_dir() or path.is_symlink():
        raise DevkitBuildError(f"{label} directory is missing: {path}")
    return path.resolve()


def _regular_file(path: Path, label: str) -> Path:
    if not path.is_file() or path.is_symlink():
        raise DevkitBuildError(f"{label} is missing: {path}")
    return path.resolve()


def _overlap(left: Path, right: Path) -> bool:
    return left == right or left.is_relative_to(right) or right.is_relative_to(left)


def _validate_tree(root: Path, label: str) -> None:
    for path in root.rglob("*"):
        if path.is_symlink():
            raise DevkitBuildError(f"{label} contains a symbolic link: {path}")


def _validated_options(
    options: DevkitBuildOptions,
) -> tuple[
    Path,
    Path,
    Path,
    Path,
    Path,
    Path,
    Path,
    Path,
    Path,
    Path | None,
]:
    repository = _directory(options.repository, "Repository")
    windows_payload = _directory(options.windows_payload, "Windows payload")
    skill_source = _directory(options.skill_source, "Skill source")
    pack_source = _directory(options.pack_source, "PostgreSQL PACK source")
    devkit_source = _directory(options.devkit_source, "Devkit source")
    inputs_lock = _regular_file(options.inputs_lock, "PostgreSQL input lock")
    wheelhouse = _directory(options.wheelhouse, "PostgreSQL wheelhouse")
    uv_archive = _regular_file(options.uv_archive, "uv archive")
    vc_redist_archive = _regular_file(
        options.vc_redist_archive, "VC Runtime archive"
    )
    output = options.output.resolve()
    archive = options.archive.resolve() if options.archive is not None else None
    if output.exists() or output.is_symlink():
        raise DevkitBuildError(f"output already exists: {output}")
    if archive is not None and (archive.exists() or archive.is_symlink()):
        raise DevkitBuildError(f"archive already exists: {archive}")
    if archive is not None:
        archive_checksum = archive.with_name(f"{archive.name}.sha256")
        if archive_checksum.exists() or archive_checksum.is_symlink():
            raise DevkitBuildError(
                f"archive checksum already exists: {archive_checksum}"
            )
    if archive is not None and archive.suffix.casefold() != ".zip":
        raise DevkitBuildError("devkit archive must use the .zip suffix")
    if (
        not options.windows_payload_provenance.strip()
        or options.windows_payload_provenance
        != options.windows_payload_provenance.strip()
        or any(
            ord(character) < 32
            for character in options.windows_payload_provenance
        )
    ):
        raise DevkitBuildError(
            "Windows Platform Payload provenance must be one non-empty line"
        )
    sources = (
        ("Windows payload", windows_payload),
        ("Skill source", skill_source),
        ("PostgreSQL PACK source", pack_source),
        ("Devkit source", devkit_source),
        ("PostgreSQL wheelhouse", wheelhouse),
    )
    for index, (label, source) in enumerate(sources):
        _validate_tree(source, label)
        if _overlap(source, output):
            raise DevkitBuildError(f"{label} overlaps output: {source} and {output}")
        for other_label, other_source in sources[index + 1 :]:
            if _overlap(source, other_source):
                raise DevkitBuildError(
                    f"devkit inputs overlap: {label} {source} and "
                    f"{other_label} {other_source}"
                )
        if archive is not None:
            archive_checksum = archive.with_name(f"{archive.name}.sha256")
            if _overlap(source, archive) or _overlap(source, archive_checksum):
                raise DevkitBuildError(
                    f"{label} overlaps archive output: {source} and {archive}"
                )
    requirements_lock = _load_inputs(inputs_lock).requirements_lock
    file_inputs = (
        ("PostgreSQL input lock", inputs_lock),
        ("PostgreSQL requirements lock", requirements_lock),
        ("uv archive", uv_archive),
        ("VC Runtime archive", vc_redist_archive),
    )
    for file_label, file_input in file_inputs:
        for source_label, source in sources:
            if file_input.is_relative_to(source):
                raise DevkitBuildError(
                    f"{file_label} is embedded in {source_label}: {file_input}"
                )
    if archive is not None:
        archive_checksum = archive.with_name(f"{archive.name}.sha256")
        if _overlap(output, archive) or _overlap(output, archive_checksum):
            raise DevkitBuildError(
                "devkit directory, archive, and archive checksum must not overlap"
            )
    return (
        repository,
        windows_payload,
        skill_source,
        pack_source,
        devkit_source,
        inputs_lock,
        wheelhouse,
        uv_archive,
        vc_redist_archive,
        archive,
    )


def _uv_environment(cache: Path) -> dict[str, str]:
    return payload_builder.isolated_environment(
        {
            "UV_CACHE_DIR": str(cache),
            "UV_LINK_MODE": "copy",
            "UV_NO_CONFIG": "1",
            "UV_NO_PROGRESS": "1",
        }
    )


def _install_postgresql_wheels(
    uv: Path,
    python: Path,
    inputs: PostgreSqlInputs,
    wheelhouse: Path,
    cache: Path,
) -> None:
    subprocess.run(
        [
            str(uv),
            "pip",
            "install",
            "--python",
            str(python),
            "--require-hashes",
            "--break-system-packages",
            "--only-binary",
            ":all:",
            "--no-config",
            "--no-index",
            "--find-links",
            str(wheelhouse),
            "--offline",
            "-r",
            str(inputs.requirements_lock),
        ],
        check=True,
        env=_uv_environment(cache),
    )
    payload_builder.check_private_host(
        uv,
        python,
        cache / "check",
        copy_links=True,
    )


def _verify_private_host(
    python: Path, inputs: PostgreSqlInputs
) -> dict[str, str | int]:
    source = (
        "import defusedxml, json, openpyxl, platform, psycopg, xlsxwriter; "
        "from kat.common.sql import postgresql as kat_common_postgresql; "
        "from psycopg import pq; "
        "print(json.dumps({'python': platform.python_version(), "
        "'psycopg': psycopg.__version__, 'pq_impl': pq.__impl__, "
        "'libpq': pq.version(), 'openpyxl': openpyxl.__version__, "
        "'xlsxwriter': xlsxwriter.__version__, "
        "'defusedxml': defusedxml.__version__, "
        "'kat_common_postgresql': kat_common_postgresql.__name__}))"
    )
    completed = subprocess.run(
        [str(python), "-I", "-B", "-X", "utf8", "-c", source],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        env=payload_builder.isolated_environment({"PSYCOPG_IMPL": "binary"}),
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise DevkitBuildError("private Host returned invalid version evidence") from error
    expected = {
        "python": inputs.python_version,
        "psycopg": inputs.psycopg_version,
        "pq_impl": "binary",
        "openpyxl": OPENPYXL_VERSION,
        "xlsxwriter": XLSXWRITER_VERSION,
        "defusedxml": DEFUSEDXML_VERSION,
        "kat_common_postgresql": PUBLIC_POSTGRESQL_CAPABILITY,
    }
    if not isinstance(result, dict) or any(result.get(k) != v for k, v in expected.items()):
        raise DevkitBuildError(f"private Host version mismatch: {result}")
    if not isinstance(result.get("libpq"), int):
        raise DevkitBuildError("private Host did not report a libpq version")
    return result


def _verify_pack_inspection(kat: Path, pack: Path, temporary_root: Path) -> None:
    data_home = temporary_root / "inspection-data-home"
    app_data = temporary_root / "inspection-app-data"
    data_home.mkdir()
    app_data.mkdir()
    environment = payload_builder.isolated_environment(
        {
            "APPDATA": str(app_data),
            "KAT_DATA_HOME": str(data_home),
            "NO_COLOR": "1",
            "PSYCOPG_IMPL": "binary",
        }
    )
    for name in tuple(environment):
        if name.upper().startswith("PG"):
            environment.pop(name)
    completed = subprocess.run(
        [
            str(kat),
            "inspect",
            "--pack",
            "postgresql-query",
            "--pack-dir",
            str(pack),
        ],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        env=environment,
    )
    try:
        response = json.loads(completed.stdout)
        result = response["result"]
        workflows = result["workflows"]
    except (json.JSONDecodeError, KeyError, TypeError) as error:
        raise DevkitBuildError("KAT inspection returned an invalid Response") from error
    if response.get("status") != "success" or result.get("name") != "postgresql-query":
        raise DevkitBuildError("KAT inspection did not select postgresql-query")
    if not isinstance(workflows, list):
        raise DevkitBuildError("KAT inspection returned invalid Workflows")
    by_name = {
        item.get("name"): item
        for item in workflows
        if isinstance(item, dict) and isinstance(item.get("name"), str)
    }
    text_workflow = by_name.get("query-postgresql")
    if (
        not isinstance(text_workflow, dict)
        or text_workflow.get("required_tables") != []
    ):
        raise DevkitBuildError(
            "query-postgresql must declare empty Required tables"
        )
    parameters = text_workflow.get("parameters")
    sql = (
        parameters[0]
        if isinstance(parameters, list) and len(parameters) == 1
        else None
    )
    if (
        not isinstance(sql, dict)
        or sql.get("name") != "sql"
        or sql.get("type") != "string"
        or sql.get("required") is not True
    ):
        raise DevkitBuildError(
            "query-postgresql must require exactly one string sql parameter"
        )
    file_workflow = by_name.get("query-postgresql-file")
    if (
        not isinstance(file_workflow, dict)
        or file_workflow.get("required_tables") != []
    ):
        raise DevkitBuildError(
            "query-postgresql-file must declare empty Required tables"
        )
    if file_workflow.get("parameters") != []:
        raise DevkitBuildError(
            "query-postgresql-file must not declare Workflow parameters"
        )


def _write_manifest(
    root: Path,
    *,
    windows_payload_provenance: str,
    inputs: PostgreSqlInputs,
    host: dict[str, str | int],
) -> None:
    kat = root / "skill/scripts/targets/windows-x86_64/kat.exe"
    manifest = {
        "schemaVersion": 1,
        "platform": build_windows_payload.PLATFORM,
        "windowsPlatformPayloadProvenance": windows_payload_provenance,
        "katExecutableSha256": _sha256(kat),
        "pythonVersion": host["python"],
        "psycopgVersion": host["psycopg"],
        "psycopgImplementation": host["pq_impl"],
        "libpqVersion": host["libpq"],
        "openpyxlVersion": host["openpyxl"],
        "xlsxwriterVersion": host["xlsxwriter"],
        "defusedxmlVersion": host["defusedxml"],
        "publicCapabilities": [host["kat_common_postgresql"]],
        "pack": "postgresql-query",
        "workflow": "query-postgresql",
        "workflows": ["query-postgresql", "query-postgresql-file"],
        "wheels": [
            {"filename": wheel.filename, "sha256": wheel.sha256}
            for wheel in inputs.wheels
        ],
    }
    (root / "DEVKIT-MANIFEST.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def _write_checksums(root: Path) -> None:
    checksum = root / "SHA256SUMS"
    files = sorted(
        path for path in root.rglob("*") if path.is_file() and path != checksum
    )
    lines = [f"{_sha256(path)}  {path.relative_to(root).as_posix()}" for path in files]
    checksum.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def _write_zip(root: Path, archive_path: Path, *, root_name: str) -> None:
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    partial = archive_path.with_name(f".{archive_path.name}.{os.getpid()}.partial")
    partial.unlink(missing_ok=True)
    try:
        with zipfile.ZipFile(
            partial,
            "w",
            compression=zipfile.ZIP_DEFLATED,
            compresslevel=9,
            strict_timestamps=False,
        ) as archive:
            for path in sorted(root.rglob("*")):
                relative = Path(root_name) / path.relative_to(root)
                if path.is_dir():
                    archive.writestr(relative.as_posix().rstrip("/") + "/", b"")
                else:
                    archive.write(path, relative.as_posix())
        partial.replace(archive_path)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise


def build_devkit(options: DevkitBuildOptions) -> Path:
    """从完整 Windows Payload 副本发布一个离线 PACK 开发包。"""

    payload_builder.require_builder_python("Windows PostgreSQL devkit")
    build_windows_payload.require_windows_builder()
    (
        repository,
        windows_payload,
        skill_source,
        pack_source,
        devkit_source,
        inputs_lock,
        wheelhouse,
        uv_archive,
        vc_redist_archive,
        archive,
    ) = _validated_options(options)
    output = options.output.resolve()
    inputs = validate_wheelhouse(inputs_lock, wheelhouse)
    windows_inputs = build_windows_payload.load_inputs(repository)
    if inputs.python_version != windows_inputs.python_version:
        raise DevkitBuildError(
            "PostgreSQL input lock does not match the Windows Host Python"
        )
    uv_archive = payload_builder.resolve_locked_asset(
        windows_inputs.uv.archive,
        uv_archive,
        output.parent / ".unused-download-cache",
        True,
    )
    vc_redist_archive = payload_builder.resolve_locked_asset(
        windows_inputs.vc_runtime.archive,
        vc_redist_archive,
        output.parent / ".unused-download-cache",
        True,
    )
    build_windows_payload.assert_payload_shape(windows_payload)

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".kpg-", dir=output.parent
    ) as temporary:
        temporary_root = Path(temporary)
        staging = temporary_root / "d"
        shutil.copytree(devkit_source, staging)
        shutil.copytree(skill_source, staging / "skill")
        staged_payload = staging / "skill/scripts/targets/windows-x86_64"
        staged_payload.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(windows_payload, staged_payload)
        shutil.copytree(pack_source, staging / "pack")
        (staging / "data-home").mkdir()
        locks = staging / "offline-locks"
        locks.mkdir()
        shutil.copy2(inputs_lock, locks / inputs_lock.name)
        shutil.copy2(inputs.requirements_lock, locks / inputs.requirements_lock.name)

        extracted_uv = temporary_root / "uv"
        payload_builder.safe_extract_zip(uv_archive, extracted_uv)
        uv = payload_builder.find_uv(extracted_uv, windows_inputs.uv.layout)
        if payload_builder.uv_version(uv) != windows_inputs.uv.version:
            raise DevkitBuildError(
                f"Windows devkit requires uv {windows_inputs.uv.version}"
            )
        python = payload_builder.private_python(
            staged_payload, build_windows_payload.PLATFORM_SPEC
        )
        _install_postgresql_wheels(
            uv,
            python,
            inputs,
            wheelhouse,
            temporary_root / "uv-cache",
        )
        payload_builder.prune_private_host(
            staged_payload / "python", build_windows_payload.PLATFORM_SPEC
        )

        extracted_redist = temporary_root / "vc-redist"
        payload_builder.safe_extract_zip(vc_redist_archive, extracted_redist)
        redist_root = build_windows_payload.resolve_vc_redist_root(
            extracted_redist, windows_inputs.vc_runtime
        )
        build_windows_payload.collect_and_copy_vc_runtime(
            staged_payload, redist_root
        )
        build_windows_payload.assert_payload_shape(staged_payload)
        host = _verify_private_host(python, inputs)
        _verify_pack_inspection(
            staged_payload / "kat.exe", staging / "pack", temporary_root
        )
        _write_manifest(
            staging,
            windows_payload_provenance=options.windows_payload_provenance,
            inputs=inputs,
            host=host,
        )
        _write_checksums(staging)
        if archive is None:
            staging.rename(output)
        else:
            archive.parent.mkdir(parents=True, exist_ok=True)
            descriptor, archive_staging_name = tempfile.mkstemp(
                prefix=f".{archive.name}.",
                suffix=".staged",
                dir=archive.parent,
            )
            os.close(descriptor)
            archive_staging = Path(archive_staging_name)
            archive_staging.unlink()
            checksum = archive.with_name(f"{archive.name}.sha256")
            descriptor, checksum_staging_name = tempfile.mkstemp(
                prefix=f".{checksum.name}.",
                suffix=".staged",
                dir=checksum.parent,
            )
            os.close(descriptor)
            checksum_staging = Path(checksum_staging_name)
            published_archive = False
            published_checksum = False
            try:
                _write_zip(staging, archive_staging, root_name=output.name)
                checksum_staging.write_text(
                    f"{_sha256(archive_staging)}  {archive.name}\n",
                    encoding="ascii",
                    newline="\n",
                )
                archive_staging.rename(archive)
                published_archive = True
                checksum_staging.rename(checksum)
                published_checksum = True
                staging.rename(output)
            except BaseException:
                if published_checksum:
                    checksum.unlink(missing_ok=True)
                if published_archive:
                    archive.unlink(missing_ok=True)
                raise
            finally:
                archive_staging.unlink(missing_ok=True)
                checksum_staging.unlink(missing_ok=True)
    return output


def _parser() -> argparse.ArgumentParser:
    repository = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description="Build a relocatable Windows PostgreSQL External PACK devkit"
    )
    parser.add_argument("--repository", type=Path, default=repository)
    parser.add_argument("--windows-payload", type=Path, required=True)
    parser.add_argument("--uv-archive", type=Path, required=True)
    parser.add_argument("--vc-redist-archive", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--windows-payload-provenance", required=True)
    return parser


def parse_args(argv: Iterable[str] | None = None) -> DevkitBuildOptions:
    arguments = _parser().parse_args(argv)
    repository = arguments.repository.resolve()
    return DevkitBuildOptions(
        repository=repository,
        windows_payload=arguments.windows_payload,
        skill_source=repository / "kat/skill",
        pack_source=repository / "examples/packs/postgresql-query",
        devkit_source=repository / "examples/postgresql-pack-devkit",
        inputs_lock=repository / "build/postgresql-pack-devkit-inputs.json",
        wheelhouse=arguments.wheelhouse,
        uv_archive=arguments.uv_archive,
        vc_redist_archive=arguments.vc_redist_archive,
        output=arguments.output,
        archive=arguments.archive,
        windows_payload_provenance=arguments.windows_payload_provenance,
    )


def main(argv: Iterable[str] | None = None) -> int:
    try:
        output = build_devkit(parse_args(argv))
    except (
        DevkitBuildError,
        OSError,
        ValueError,
        subprocess.CalledProcessError,
        zipfile.BadZipFile,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
