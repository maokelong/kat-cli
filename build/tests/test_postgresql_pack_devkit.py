from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import sys
import tempfile
import unittest
import zipfile
from dataclasses import replace
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
MODULE_PATH = REPOSITORY / "build/build_postgresql_pack_devkit.py"
SPEC = importlib.util.spec_from_file_location("build_postgresql_pack_devkit", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
postgresql_devkit = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = postgresql_devkit
SPEC.loader.exec_module(postgresql_devkit)


def sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def write_file(path: Path, content: bytes = b"fixture") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def write_zip(path: Path, members: dict[str, bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as archive:
        for name, content in members.items():
            archive.writestr(name, content)


def write_repository_inputs(repository: Path, uv_archive: Path, vc_archive: Path) -> None:
    write_file(repository / "build/requirements-windows.lock.txt", b"base==1\n")
    (repository / "build/runtime-inputs.json").write_text(
        json.dumps(
            {
                "schemaVersion": 2,
                "python": {
                    "implementation": "CPython",
                    "version": "3.14.6",
                    "abi": "standard-gil",
                    "provider": "python-build-standalone",
                    "release": "20260623",
                },
                "uv": {"version": "0.11.28"},
                "platforms": {
                    "windows-x86_64": {
                        "rustTarget": "x86_64-pc-windows-msvc",
                        "minimumWindows": "10",
                        "pythonArchive": {
                            "filename": "cpython-3.14.6+20260623-x86_64-pc-windows-msvc-install_only_stripped.tar.gz",
                            "url": "https://example.invalid/python.tar.gz",
                            "sha256": "0" * 64,
                        },
                        "uvArchive": {
                            "filename": uv_archive.name,
                            "url": "https://example.invalid/uv.zip",
                            "sha256": hashlib.sha256(uv_archive.read_bytes()).hexdigest(),
                        },
                        "uvLayout": {
                            "archiveFormat": "zip",
                            "executable": "uv.exe",
                            "needsExecutableBit": False,
                        },
                        "vcRuntime": {
                            "provider": "Visual Studio 2022 CRT Redist VSIX",
                            "archive": {
                                "filename": vc_archive.name,
                                "url": "https://example.invalid/vc.vsix",
                                "sha256": hashlib.sha256(vc_archive.read_bytes()).hexdigest(),
                            },
                            "contentRoot": "Contents/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT",
                        },
                        "requirementsLock": "build/requirements-windows.lock.txt",
                    }
                },
            }
        ),
        encoding="utf-8",
    )


class PostgreSqlPackDevkitBuilderTests(unittest.TestCase):
    def test_tampered_postgresql_wheel_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheelhouse = root / "wheelhouse"
            wheelhouse.mkdir()
            good_content = b"locked wheel"
            wheel = wheelhouse / "psycopg-3.3.4-py3-none-any.whl"
            wheel.write_bytes(b"tampered wheel")
            inputs = root / "postgresql-pack-devkit-inputs.json"
            requirements = root / "requirements-postgresql-windows.lock.txt"
            requirements.write_text("psycopg==3.3.4\n", encoding="utf-8")
            inputs.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "platform": "windows-x86_64",
                        "pythonVersion": "3.14.6",
                        "psycopgVersion": "3.3.4",
                        "requirementsLock": requirements.name,
                        "wheels": [
                            {
                                "filename": wheel.name,
                                "sha256": sha256(good_content),
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                postgresql_devkit.DevkitBuildError,
                "SHA-256",
            ):
                postgresql_devkit.validate_wheelhouse(inputs, wheelhouse)

    def test_build_publishes_one_relocatable_skill_pack_and_checksum_view(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository = root / "repository"
            uv_archive = root / "uv-x86_64-pc-windows-msvc.zip"
            write_zip(uv_archive, {"uv.exe": b"locked uv"})
            vc_archive = root / "Microsoft.VC.Redist.X64.base.vsix"
            write_zip(
                vc_archive,
                {
                    "Contents/VC/Redist/MSVC/14.44.35112/x64/"
                    "Microsoft.VC143.CRT/runtime.dll": b"locked runtime"
                },
            )
            write_repository_inputs(repository, uv_archive, vc_archive)

            skill_source = repository / "kat/skill"
            write_file(skill_source / "SKILL.md", b"# KAT\n")
            pack_source = repository / "examples/packs/postgresql-query"
            write_file(
                pack_source / "pack.toml",
                b'name = "postgresql-query"\n',
            )
            write_file(pack_source / "workflows/query_postgresql.py", b"# workflow\n")
            devkit_source = repository / "examples/postgresql-pack-devkit"
            write_file(devkit_source / "README.md", b"# Devkit\n")
            write_file(devkit_source / "ENVIRONMENT.md", b"# Environment\n")
            write_file(devkit_source / "scripts/Verify-Devkit.ps1", b"# verify\n")
            write_file(devkit_source / "queries/smoke.sql", b"SELECT 1;\n")

            payload = root / "windows-payload"
            write_file(payload / "kat.exe", b"kat")
            write_file(payload / "python/python.exe", b"python")
            os.utime(payload / "python/python.exe", (1, 1))
            payload_snapshot = {
                path.relative_to(payload): path.read_bytes()
                for path in payload.rglob("*")
                if path.is_file()
            }

            wheelhouse = root / "wheelhouse"
            wheel_contents = {
                "psycopg-3.3.4-py3-none-any.whl": b"psycopg",
                "psycopg_binary-3.3.4-cp314-cp314-win_amd64.whl": b"binary",
                "tzdata-2026.3-py2.py3-none-any.whl": b"tzdata",
            }
            for name, content in wheel_contents.items():
                write_file(wheelhouse / name, content)
            requirements_lock = repository / "build/requirements-postgresql-windows.lock.txt"
            write_file(requirements_lock, b"locked requirements\n")
            inputs_lock = repository / "build/postgresql-pack-devkit-inputs.json"
            inputs_lock.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "platform": "windows-x86_64",
                        "pythonVersion": "3.14.6",
                        "psycopgVersion": "3.3.4",
                        "requirementsLock": requirements_lock.name,
                        "wheels": [
                            {"filename": name, "sha256": sha256(content)}
                            for name, content in wheel_contents.items()
                        ],
                    }
                ),
                encoding="utf-8",
            )

            output = repository / "target/kat/postgresql-pack-devkit"
            archive = repository / "target/kat/postgresql-pack-devkit-windows-x86_64.zip"
            commands: list[list[str]] = []
            command_options: list[dict[str, object]] = []

            def run(command: list[str], **options: object) -> mock.Mock:
                commands.append(command)
                command_options.append(options)
                if command[1:] == ["--version"]:
                    return mock.Mock(stdout="uv 0.11.28\n", returncode=0)
                if "-c" in command:
                    return mock.Mock(
                        stdout=json.dumps(
                            {
                                "python": "3.14.6",
                                "psycopg": "3.3.4",
                                "pq_impl": "binary",
                                "libpq": 180003,
                            }
                        )
                        + "\n",
                        returncode=0,
                    )
                if command[0].endswith("kat.exe"):
                    return mock.Mock(
                        stdout=json.dumps(
                            {
                                "status": "success",
                                "result": {
                                    "name": "postgresql-query",
                                    "workflows": [
                                        {
                                            "name": "query-postgresql",
                                            "required_tables": [],
                                            "parameters": [
                                                {
                                                    "name": "sql",
                                                    "type": "string",
                                                    "required": True,
                                                }
                                            ],
                                        }
                                    ],
                                },
                            }
                        )
                        + "\n",
                        returncode=0,
                    )
                return mock.Mock(stdout="", returncode=0)

            options = postgresql_devkit.DevkitBuildOptions(
                repository=repository,
                windows_payload=payload,
                skill_source=skill_source,
                pack_source=pack_source,
                devkit_source=devkit_source,
                inputs_lock=inputs_lock,
                wheelhouse=wheelhouse,
                uv_archive=uv_archive,
                vc_redist_archive=vc_archive,
                output=output,
                archive=archive,
                windows_payload_provenance="fixture Windows Platform Payload",
            )
            with (
                mock.patch.object(postgresql_devkit.subprocess, "run", side_effect=run),
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "require_windows_builder",
                ),
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "collect_and_copy_vc_runtime",
                ),
            ):
                published = postgresql_devkit.build_devkit(options)

            self.assertEqual(published, output)
            self.assertTrue((output / "skill/SKILL.md").is_file())
            self.assertTrue(
                (
                    output
                    / "skill/scripts/targets/windows-x86_64/kat.exe"
                ).is_file()
            )
            self.assertTrue((output / "pack/pack.toml").is_file())
            self.assertTrue((output / "data-home").is_dir())
            self.assertTrue((output / "README.md").is_file())
            self.assertTrue((output / "ENVIRONMENT.md").is_file())
            self.assertTrue((output / "DEVKIT-MANIFEST.json").is_file())
            manifest = json.loads(
                (output / "DEVKIT-MANIFEST.json").read_text("utf-8")
            )
            self.assertEqual(
                manifest["windowsPlatformPayloadProvenance"],
                "fixture Windows Platform Payload",
            )
            self.assertTrue((output / "SHA256SUMS").is_file())
            self.assertTrue(archive.is_file())
            archive_checksum = archive.with_name(f"{archive.name}.sha256")
            self.assertEqual(
                archive_checksum.read_text("ascii").split(),
                [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name],
            )
            with zipfile.ZipFile(archive) as built_archive:
                self.assertIn(
                    f"{output.name}/DEVKIT-MANIFEST.json",
                    built_archive.namelist(),
                )
            install = next(command for command in commands if command[1:3] == ["pip", "install"])
            self.assertIn("--no-index", install)
            self.assertIn("--require-hashes", install)
            self.assertNotIn("sync", install)
            self.assertIn(
                str(output.parent),
                str(Path(install[install.index("--python") + 1]).parent.parent.parent),
            )
            inspection_index = next(
                index
                for index, command in enumerate(commands)
                if command[0].endswith("kat.exe")
            )
            self.assertEqual(command_options[inspection_index]["encoding"], "utf-8")
            self.assertEqual(
                {
                    path.relative_to(payload): path.read_bytes()
                    for path in payload.rglob("*")
                    if path.is_file()
                },
                payload_snapshot,
            )

            with (
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "require_windows_builder",
                ),
                self.assertRaisesRegex(
                    postgresql_devkit.DevkitBuildError,
                    "output already exists",
                ),
            ):
                postgresql_devkit.build_devkit(options)

            overlap_options = replace(
                options,
                output=payload / "nested-output",
                archive=None,
            )
            with (
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "require_windows_builder",
                ),
                self.assertRaisesRegex(
                    postgresql_devkit.DevkitBuildError,
                    "overlaps output",
                ),
            ):
                postgresql_devkit.build_devkit(overlap_options)

            wrong_uv_output = repository / "target/kat/wrong-uv-devkit"
            wrong_uv_options = replace(
                options,
                output=wrong_uv_output,
                archive=None,
            )
            with (
                mock.patch.object(
                    postgresql_devkit.subprocess,
                    "run",
                    side_effect=run,
                ),
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "require_windows_builder",
                ),
                mock.patch.object(
                    postgresql_devkit.payload_builder,
                    "uv_version",
                    return_value="0.0.0",
                ),
                self.assertRaisesRegex(
                    postgresql_devkit.DevkitBuildError,
                    "requires uv 0.11.28",
                ),
            ):
                postgresql_devkit.build_devkit(wrong_uv_options)
            self.assertFalse(wrong_uv_output.exists())

            failed_output = repository / "target/kat/archive-failure-devkit"
            failed_archive = repository / "target/kat/archive-failure-devkit.zip"
            failed_options = replace(
                options,
                output=failed_output,
                archive=failed_archive,
            )
            with (
                mock.patch.object(
                    postgresql_devkit.subprocess,
                    "run",
                    side_effect=run,
                ),
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "require_windows_builder",
                ),
                mock.patch.object(
                    postgresql_devkit.build_windows_payload,
                    "collect_and_copy_vc_runtime",
                ),
                mock.patch.object(
                    postgresql_devkit,
                    "_write_zip",
                    side_effect=OSError("simulated archive failure"),
                ),
                self.assertRaisesRegex(OSError, "simulated archive failure"),
            ):
                postgresql_devkit.build_devkit(failed_options)
            self.assertFalse(failed_output.exists())
            self.assertFalse(failed_archive.exists())
            self.assertFalse(
                failed_archive.with_name(f"{failed_archive.name}.sha256").exists()
            )

    def test_private_host_version_mismatch_is_rejected(self) -> None:
        inputs = postgresql_devkit.PostgreSqlInputs(
            platform="windows-x86_64",
            python_version="3.14.6",
            psycopg_version="3.3.4",
            requirements_lock=Path("requirements.txt"),
            wheels=(),
        )
        expected = {
            "python": "3.14.6",
            "psycopg": "3.3.4",
            "pq_impl": "binary",
            "libpq": 180003,
        }
        for field, wrong_value in (
            ("python", "3.14.5"),
            ("psycopg", "3.3.3"),
            ("pq_impl", "python"),
        ):
            with self.subTest(field=field):
                result = {**expected, field: wrong_value}
                completed = mock.Mock(
                    stdout=json.dumps(result),
                    returncode=0,
                )
                with (
                    mock.patch.object(
                        postgresql_devkit.subprocess,
                        "run",
                        return_value=completed,
                    ),
                    self.assertRaisesRegex(
                        postgresql_devkit.DevkitBuildError,
                        "version mismatch",
                    ),
                ):
                    postgresql_devkit._verify_private_host(
                        Path("python.exe"), inputs
                    )


if __name__ == "__main__":
    unittest.main()
