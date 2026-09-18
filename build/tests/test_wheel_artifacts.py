from __future__ import annotations
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import payload_builder
import build_linux_payload
import build_windows_payload


def write_sdk_wheel(path, *, version="0.1.1rc1", tag="cp314-cp314-win_amd64",
                    extension="kat/providers/_native.cp314-win_amd64.pyd", requires=None):
    info = f"kat_sdk-{version}.dist-info"
    with zipfile.ZipFile(path, "w") as archive:
        for name in ("__init__.py", "_declarations/workflow.py", "_declarations/provider.py",
                     "dataprovider/__init__.py", "_runtime/__main__.py", "knowledge/index.md",
                     "knowledge/authoring/pack-authoring-flow.md",
                     "knowledge/providers/ftrace/guide.md", "knowledge/providers/trace_streamer/guide.md",
                     "providers/decoding/__init__.py", "providers/decoding/hitrace.py",
                     "providers/decoding/text_ftrace.py"):
            archive.writestr("kat/" + name, "fixture")
        archive.writestr(extension, b"native extension")
        metadata = f"Metadata-Version: 2.4\nName: kat-sdk\nVersion: {version}\n"
        if requires:
            metadata += f"Requires-Dist: {requires}\n"
        archive.writestr(f"{info}/METADATA", metadata)
        archive.writestr(f"{info}/WHEEL", f"Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: {tag}\n")


class WheelArtifactTests(unittest.TestCase):
    def test_version_and_digest_are_explicit_and_verified(self):
        with self.assertRaises(TypeError):
            payload_builder.validated_sdk_wheel(Path("sdk.whl"), platform="windows-x86_64")
        with tempfile.TemporaryDirectory() as directory:
            wheel = Path(directory) / "kat_sdk-0.1.1rc1-cp314-cp314-win_amd64.whl"
            write_sdk_wheel(wheel)
            digest = payload_builder.file_sha256(wheel)
            artifact = payload_builder.WheelArtifactInput(wheel, "0.1.1rc1", digest)
            self.assertEqual(payload_builder.validated_sdk_wheel(artifact, platform="windows-x86_64"), wheel.resolve())
            for version, sha in (("0.1.2", digest), ("0.1.1rc1", "0" * 64), ("0.1.1rc1", "invalid")):
                with self.subTest(version=version, sha=sha), self.assertRaises(ValueError):
                    payload_builder.validated_sdk_wheel(payload_builder.WheelArtifactInput(wheel, version, sha), platform="windows-x86_64")

    def test_platform_identity_and_single_extension(self):
        cases = (("windows-x86_64", "cp314-cp314-win_amd64", "kat/providers/_native.cp314-win_amd64.pyd"),
                 ("linux-x86_64", "cp314-cp314-manylinux_2_28_x86_64", "kat/providers/_native.cpython-314-x86_64-linux-gnu.so"))
        with tempfile.TemporaryDirectory() as directory:
            for platform, tag, extension in cases:
                wheel = Path(directory) / f"kat_sdk-0.1.1rc1-{tag}.whl"
                write_sdk_wheel(wheel, tag=tag, extension=extension)
                payload_builder.validate_sdk_wheel_archive(wheel, expected_version="0.1.1rc1", platform=platform)
                with zipfile.ZipFile(wheel, "a") as archive:
                    archive.writestr("kat/providers/other.pyd", b"extra")
                with self.assertRaisesRegex(ValueError, "exactly one"):
                    payload_builder.validate_sdk_wheel_archive(wheel, expected_version="0.1.1rc1", platform=platform)
            wheel = Path(directory) / "kat_sdk-0.1.1rc1-cp314-cp314-win_amd64.whl"
            write_sdk_wheel(wheel, tag="cp314-cp314-manylinux_2_28_x86_64")
            with self.assertRaisesRegex(ValueError, "tag"):
                payload_builder.validate_sdk_wheel_archive(wheel, expected_version="0.1.1rc1", platform="windows-x86_64")

    def test_payload_installs_one_sdk_without_resolving_dependencies(self):
        with mock.patch.object(payload_builder.subprocess, "run") as run:
            payload_builder.install_kat_wheels(Path("uv"), Path("python"), (Path("sdk.whl"),), Path("cache"), copy_links=False)
        self.assertEqual(run.call_count, 1)
        command = run.call_args.args[0]
        self.assertIn("--no-deps", command)
        self.assertIn("--no-index", command)
        self.assertEqual(command[-1], "sdk.whl")

    def test_platform_builders_require_explicit_sdk_artifact(self):
        for module in (build_linux_payload, build_windows_payload):
            options = module.parse_args(["--sdk-wheel", "sdk.whl", "--sdk-wheel-version", "0.1.1rc1", "--sdk-wheel-sha256", "a" * 64])
            self.assertEqual(options.sdk_wheel, payload_builder.WheelArtifactInput(Path("sdk.whl"), "0.1.1rc1", "a" * 64))

    def test_invalid_artifact_fails_before_payload_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheel = root / "kat_sdk-0.1.1rc1-cp314-cp314-win_amd64.whl"
            write_sdk_wheel(wheel)
            options = mock.Mock(repository=root, output=root / "payload", download_cache=root / "downloads",
                                sdk_wheel=payload_builder.WheelArtifactInput(wheel, "0.1.1rc1", "0" * 64),
                                wheelhouse=None, python_archive=None, uv_archive=None, offline=False)
            adapter = mock.Mock()
            adapter.spec = mock.Mock(key="windows-x86_64", label="Windows")
            adapter.extra_input_paths.return_value = ()
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                payload_builder.build_payload(options, adapter)
            self.assertFalse(options.output.exists())
            adapter.resolve_extra_inputs.assert_not_called()
