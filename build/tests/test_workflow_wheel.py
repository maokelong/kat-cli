from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
MODULE_PATH = REPOSITORY / "build/build_workflow_wheel.py"
SPEC = importlib.util.spec_from_file_location("build_workflow_wheel", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
workflow_wheel = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = workflow_wheel
SPEC.loader.exec_module(workflow_wheel)


WHEEL_NAME = "kat_workflow-9.7.0-py3-none-any.whl"


def write_wheel(
    path: Path,
    *,
    version: str = "9.7.0",
    distribution: str = "kat-workflow",
    metadata_version: str | None = None,
    tag: str = "py3-none-any",
) -> None:
    dist_info = f"kat_workflow-{version}.dist-info"
    metadata_version = version if metadata_version is None else metadata_version
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("kat/__init__.py", "")
        archive.writestr("_kat_runtime/__main__.py", "")
        archive.writestr(
            f"{dist_info}/METADATA",
            f"Metadata-Version: 2.4\nName: {distribution}\nVersion: {metadata_version}\n",
        )
        archive.writestr(
            f"{dist_info}/WHEEL",
            f"Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: {tag}\n",
        )


class WorkflowWheelTests(unittest.TestCase):
    def test_downloaded_uv_uses_the_linux_platform_spec(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            uv = root / "uv"
            uv.write_bytes(b"locked uv")
            archive = root / "uv.tar.gz"
            archive.write_bytes(b"locked archive")

            def fake_run(command: list[str], **_: object) -> mock.Mock:
                if command[1] == "--version":
                    return mock.Mock(stdout="uv 0.11.28\n")
                out_dir = Path(command[command.index("--out-dir") + 1])
                write_wheel(out_dir / WHEEL_NAME)
                return mock.Mock()

            with (
                mock.patch.object(
                    workflow_wheel.payload_builder,
                    "resolve_locked_asset",
                    return_value=archive,
                ),
                mock.patch.object(workflow_wheel.payload_builder, "safe_extract_tar"),
                mock.patch.object(
                    workflow_wheel.payload_builder,
                    "find_uv",
                    return_value=uv,
                ) as find_uv,
                mock.patch.object(
                    workflow_wheel.subprocess, "run", side_effect=fake_run
                ),
            ):
                workflow_wheel.build_workflow_wheel(REPOSITORY, None, root / "wheel")

            self.assertIs(
                find_uv.call_args.args[1],
                workflow_wheel.build_linux_payload.PLATFORM_SPEC,
            )

    def test_build_uses_locked_uv_and_writes_a_required_checksum(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            uv = root / "uv"
            uv.write_bytes(b"locked uv")
            output = root / "wheel"

            def fake_run(command: list[str], **arguments: object) -> mock.Mock:
                if command[1] == "--version":
                    return mock.Mock(stdout="uv 0.11.28\n")
                self.assertEqual(command[1:3], ["build", "--wheel"])
                environment = arguments["env"]
                assert isinstance(environment, dict)
                self.assertEqual(environment["SOURCE_DATE_EPOCH"], "315532800")
                out_dir = Path(command[command.index("--out-dir") + 1])
                write_wheel(out_dir / WHEEL_NAME)
                return mock.Mock()

            with mock.patch.object(workflow_wheel.subprocess, "run", side_effect=fake_run):
                wheel, checksum = workflow_wheel.build_workflow_wheel(
                    REPOSITORY, uv, output
                )

            self.assertEqual(
                {path.name for path in output.iterdir()},
                {WHEEL_NAME, f"{WHEEL_NAME}.sha256"},
            )
            self.assertIn(workflow_wheel.file_sha256(wheel), checksum.read_text("ascii"))
            self.assertEqual(
                workflow_wheel.payload_builder.validated_workflow_wheel(wheel),
                wheel.resolve(),
            )
            self.assertEqual(
                workflow_wheel.payload_builder.find_workflow_wheel(output), wheel
            )

            with self.subTest(case="missing wheel"):
                empty = root / "empty"
                empty.mkdir()
                with self.assertRaisesRegex(ValueError, "found 0"):
                    workflow_wheel.payload_builder.find_workflow_wheel(empty)

            with self.subTest(case="multiple wheels"):
                second = output / "kat_workflow-9.8.0-py3-none-any.whl"
                write_wheel(second, version="9.8.0")
                with self.assertRaisesRegex(ValueError, "found 2"):
                    workflow_wheel.payload_builder.find_workflow_wheel(output)
                second.unlink()

            with self.subTest(case="unexpected distribution"):
                invalid = root / WHEEL_NAME
                write_wheel(invalid, distribution="other")
                with self.assertRaisesRegex(ValueError, "distribution"):
                    workflow_wheel.payload_builder.validate_workflow_wheel_archive(
                        invalid
                    )

            with self.subTest(case="unexpected tag"):
                invalid = root / WHEEL_NAME
                write_wheel(invalid, tag="cp314-cp314-win_amd64")
                with self.assertRaisesRegex(ValueError, "py3-none-any"):
                    workflow_wheel.payload_builder.validate_workflow_wheel_archive(
                        invalid
                    )

            with self.subTest(case="version mismatch"):
                invalid = root / WHEEL_NAME
                write_wheel(invalid, metadata_version="9.8.0")
                with self.assertRaisesRegex(ValueError, "version"):
                    workflow_wheel.payload_builder.validate_workflow_wheel_archive(
                        invalid
                    )

            wheel.write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                workflow_wheel.payload_builder.validated_workflow_wheel(wheel)


if __name__ == "__main__":
    unittest.main()
