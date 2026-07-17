from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
MODULE_PATH = REPOSITORY / "build/build_workflow_wheel.py"
SPEC = importlib.util.spec_from_file_location("build_workflow_wheel", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
workflow_wheel = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = workflow_wheel
SPEC.loader.exec_module(workflow_wheel)


def write_wheel(path: Path, *, pure: bool = True) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("kat/__init__.py", "")
        archive.writestr("_kat_runtime/__main__.py", "")
        archive.writestr("kat_workflow-0.1.0.dist-info/METADATA", "Version: 0.1.0\n")
        archive.writestr(
            "kat_workflow-0.1.0.dist-info/WHEEL",
            f"Root-Is-Purelib: {'true' if pure else 'false'}\n",
        )


class WorkflowWheelTests(unittest.TestCase):
    def test_build_uses_locked_uv_and_writes_checksum(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            uv = root / "uv"
            uv.write_bytes(b"locked uv")
            output = root / "wheel"

            def fake_run(command: list[str], **arguments: object):
                if command[1] == "--version":
                    return mock.Mock(stdout="uv 0.11.28\n")
                self.assertEqual(command[1:3], ["build", "--wheel"])
                out_dir = Path(command[command.index("--out-dir") + 1])
                write_wheel(out_dir / workflow_wheel.WHEEL_NAME)
                environment = arguments["env"]
                assert isinstance(environment, dict)
                self.assertEqual(environment["SOURCE_DATE_EPOCH"], "315532800")
                return mock.Mock()

            with mock.patch.object(workflow_wheel.subprocess, "run", side_effect=fake_run):
                wheel, checksum = workflow_wheel.build_workflow_wheel(
                    REPOSITORY, uv, output
                )

            self.assertEqual(wheel.name, workflow_wheel.WHEEL_NAME)
            self.assertIn(workflow_wheel.file_sha256(wheel), checksum.read_text("ascii"))
            self.assertEqual(
                {path.name for path in output.iterdir()},
                {workflow_wheel.WHEEL_NAME, f"{workflow_wheel.WHEEL_NAME}.sha256"},
            )

    def test_rejects_non_pure_or_wrong_version_wheel(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wrong = root / "kat_workflow-0.2.0-py3-none-any.whl"
            write_wheel(wrong)
            with self.assertRaisesRegex(ValueError, "unexpected"):
                workflow_wheel.validate_wheel(wrong)

            native = root / workflow_wheel.WHEEL_NAME
            write_wheel(native, pure=False)
            with self.assertRaisesRegex(ValueError, "pure Python"):
                workflow_wheel.validate_wheel(native)


if __name__ == "__main__":
    unittest.main()
