from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
MODULE_PATH = REPOSITORY / "build/payload_builder.py"
SPEC = importlib.util.spec_from_file_location("payload_builder_excel_test", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
payload_builder = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = payload_builder
SPEC.loader.exec_module(payload_builder)


class ExcelHostTests(unittest.TestCase):
    def test_bundled_python_runs_the_excel_smoke_contract_in_isolation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "excel-smoke"
            python = Path(directory) / "payload/python/python.exe"

            with mock.patch.object(payload_builder.subprocess, "run") as run:
                payload_builder.check_excel_host(python, workspace)

            command = run.call_args.args[0]
            self.assertEqual(command[:3], [str(python), "-I", "-c"])
            self.assertEqual(command[4], str(workspace))
            self.assertTrue(run.call_args.kwargs["check"])
            self.assertEqual(
                run.call_args.kwargs["env"], payload_builder.isolated_environment()
            )
            script = command[3]
            for contract in (
                "openpyxl.xml.DEFUSEDXML",
                "openpyxl.xlsx",
                "xlsxwriter.xlsx",
                "load_workbook",
                "xlsxwriter.Workbook",
            ):
                self.assertIn(contract, script)

    def test_payload_preparation_checks_excel_after_host_pruning(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            python = root / "payload/python/python.exe"
            inputs = SimpleNamespace(
                uv=SimpleNamespace(
                    version="0.11.28",
                    layout=SimpleNamespace(archive_format="zip"),
                )
            )
            spec = SimpleNamespace(label="Windows", copy_uv_links=True)
            events: list[str] = []

            with (
                mock.patch.object(payload_builder, "safe_extract_zip"),
                mock.patch.object(payload_builder, "find_uv", return_value=root / "uv"),
                mock.patch.object(
                    payload_builder, "uv_version", return_value="0.11.28"
                ),
                mock.patch.object(payload_builder, "install_private_python"),
                mock.patch.object(
                    payload_builder, "private_python", return_value=python
                ),
                mock.patch.object(payload_builder, "install_locked_requirements"),
                mock.patch.object(payload_builder, "install_workflow_wheel"),
                mock.patch.object(payload_builder, "check_private_host"),
                mock.patch.object(
                    payload_builder,
                    "prune_private_host",
                    side_effect=lambda *_: events.append("prune"),
                ),
                mock.patch.object(
                    payload_builder,
                    "check_excel_host",
                    side_effect=lambda *_: events.append("excel"),
                ) as check_excel,
            ):
                payload_builder._prepare_private_host(
                    spec=spec,
                    stage=root / "payload",
                    temporary_root=root / "work",
                    python_archive=root / "python.tar.gz",
                    uv_archive=root / "uv.zip",
                    inputs=inputs,
                    workflow_wheel=root / "kat-workflow.whl",
                    wheelhouse=None,
                    offline=False,
                )

            self.assertEqual(events, ["prune", "excel"])
            check_excel.assert_called_once_with(python, root / "work/excel-host-smoke")


if __name__ == "__main__":
    unittest.main()
