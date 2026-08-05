from __future__ import annotations

import io
import json
import stat
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from typing import Any
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import build_linux_payload
import build_windows_payload
import payload_builder


class PayloadBuilderTests(unittest.TestCase):
    def _assert_private_python_and_requirements(
        self,
        module: Any,
    ) -> None:
        inputs = module.load_inputs(REPOSITORY)
        windows = module is build_windows_payload
        uv_name = "uv.exe" if windows else "uv"
        launcher = (
            "cpython-{version}-windows-x86_64-none/python.exe"
            if windows
            else "cpython-{version}-linux-x86_64-gnu/bin/python3"
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / inputs.python_archive.filename
            archive.write_bytes(b"locked python archive")
            wheelhouse_path = root / "wheelhouse" if windows else None
            if wheelhouse_path is not None:
                wheelhouse_path.mkdir()
            stage = root / "payload"
            temporary_root = root / "work"
            temporary_root.mkdir()
            commands: list[list[str]] = []

            def run(command: list[str], **_: object) -> mock.Mock:
                commands.append(command)
                if command[1:3] == ["python", "install"]:
                    install_dir = Path(command[command.index("--install-dir") + 1])
                    python = install_dir / launcher.format(version=inputs.python_version)
                    python.parent.mkdir(parents=True)
                    python.write_bytes(b"python")
                return mock.Mock(stdout="")

            with mock.patch.object(payload_builder.subprocess, "run", side_effect=run):
                payload_builder.install_private_python(
                    uv=root / uv_name,
                    python_archive=archive,
                    inputs=inputs,
                    stage=stage,
                    temporary_root=temporary_root,
                    spec=module.PLATFORM_SPEC,
                )
                payload_builder.install_locked_requirements(
                    root / uv_name,
                    payload_builder.private_python(stage, module.PLATFORM_SPEC),
                    inputs,
                    root / "cache",
                    wheelhouse_path,
                    windows,
                    copy_links=windows,
                )

            install = commands[0]
            for option in (
                "--python-downloads-json-url",
                "--managed-python",
                "--no-bin",
                "--no-registry",
                "--no-config",
                "--offline",
            ):
                self.assertIn(option, install)
            metadata = Path(
                install[install.index("--python-downloads-json-url") + 1]
            )
            download = json.loads(metadata.read_text(encoding="utf-8"))[
                launcher.split("/", 1)[0].format(version=inputs.python_version)
            ]
            self.assertEqual(download["url"], archive.as_uri())
            self.assertEqual(download["sha256"], inputs.python_archive.sha256)
            self.assertTrue(
                payload_builder.private_python(stage, module.PLATFORM_SPEC).is_file()
            )

            sync = commands[1]
            self.assertEqual(sync[:3], [str(root / uv_name), "pip", "sync"])
            for option in ("--require-hashes", "--only-binary", "--strict", "--no-config"):
                self.assertIn(option, sync)
            self.assertIn(str(inputs.requirements_lock), sync)
            if wheelhouse_path is not None:
                self.assertIn(str(wheelhouse_path.resolve()), sync)
                for option in ("--no-index", "--find-links", "--offline"):
                    self.assertIn(option, sync)

    def test_linux_private_python_and_requirements_use_locked_inputs(self) -> None:
        self._assert_private_python_and_requirements(build_linux_payload)

    def test_windows_private_python_and_requirements_use_locked_inputs(self) -> None:
        self._assert_private_python_and_requirements(build_windows_payload)

    def test_platform_adapters_own_platform_specific_options(self) -> None:
        linux = build_linux_payload.LinuxAdapter(readelf="locked-readelf")
        self.assertEqual(linux.readelf, "locked-readelf")
        self.assertEqual(list(linux.extra_input_paths()), [])

        vc_redist = Path("locked-vc-runtime.vsix")
        windows = build_windows_payload.WindowsAdapter(
            vc_redist_archive=vc_redist
        )
        self.assertEqual(
            list(windows.extra_input_paths()),
            [("VC Runtime VSIX", vc_redist)],
        )

    def test_archive_extraction_rejects_traversal_and_symlink_escape(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside"
            outside.mkdir()

            tar_traversal = root / "traversal.tar.gz"
            with tarfile.open(tar_traversal, "w:gz") as archive:
                member = tarfile.TarInfo("../outside/owned")
                member.size = 1
                archive.addfile(member, io.BytesIO(b"x"))
            with self.subTest(archive="tar traversal"), self.assertRaises(
                tarfile.FilterError
            ):
                payload_builder.safe_extract_tar(
                    tar_traversal, root / "tar-output", platform_label="Linux"
                )

            zip_traversal = root / "traversal.zip"
            with zipfile.ZipFile(zip_traversal, "w") as archive:
                archive.writestr("../outside/owned", b"x")
            with self.subTest(archive="zip traversal"), self.assertRaisesRegex(
                ValueError, "unsafe zip member"
            ):
                payload_builder.safe_extract_zip(zip_traversal, root / "zip-output")

            zip_symlink = root / "symlink.zip"
            link = zipfile.ZipInfo("link")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            with zipfile.ZipFile(zip_symlink, "w") as archive:
                archive.writestr(link, "../outside")
            with self.subTest(archive="zip symlink"), self.assertRaisesRegex(
                ValueError, "unsafe zip member"
            ):
                payload_builder.safe_extract_zip(zip_symlink, root / "link-output")

            tar_symlink = root / "symlink.tar.gz"
            with tarfile.open(tar_symlink, "w:gz") as archive:
                link = tarfile.TarInfo("link")
                link.type = tarfile.SYMTYPE
                link.linkname = "../outside"
                archive.addfile(link)
            with self.subTest(archive="tar symlink"), self.assertRaises(
                tarfile.FilterError
            ):
                payload_builder.safe_extract_tar(
                    tar_symlink, root / "tar-link-output", platform_label="Linux"
                )

            self.assertFalse((outside / "owned").exists())

    def test_payload_roots_expose_only_the_platform_entrypoint(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            linux = root / "linux"
            linux.mkdir()
            linux_cli = linux / "kat"
            linux_cli.write_bytes(b"cli")
            linux_cli.chmod(linux_cli.stat().st_mode | stat.S_IXUSR)
            (linux / "python/bin").mkdir(parents=True)
            (linux / "python/bin/python3").write_bytes(b"python")
            build_linux_payload.assert_payload_shape(linux)

            windows = root / "windows"
            windows.mkdir()
            (windows / "kat.exe").write_bytes(b"cli")
            (windows / "python").mkdir()
            (windows / "python/python.exe").write_bytes(b"python")
            build_windows_payload.assert_payload_shape(windows)

            for payload, validator, extra, message in (
                (linux, build_linux_payload.assert_payload_shape, "pack.toml", "root"),
                (
                    linux,
                    build_linux_payload.assert_payload_shape,
                    "python/pack.toml",
                    "artifacts",
                ),
                (
                    windows,
                    build_windows_payload.assert_payload_shape,
                    "runtime.dll",
                    "only kat.exe",
                ),
                (
                    windows,
                    build_windows_payload.assert_payload_shape,
                    "python/vc_redist.x64.exe",
                    "artifacts",
                ),
            ):
                with self.subTest(platform=payload.name, extra=extra):
                    unexpected = payload / extra
                    unexpected.write_bytes(b"unexpected")
                    with self.assertRaisesRegex(ValueError, message):
                        validator(payload)
                    unexpected.unlink()


if __name__ == "__main__":
    unittest.main()
