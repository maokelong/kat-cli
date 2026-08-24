from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

import pyarrow as pa

from _kat_runtime.request import RuntimeRequestError, read_request
from _source_dataset import materialized_dataset_request, write_materialized_source


class PackTestingProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(
        self,
        request: object,
        pack: Path,
        *,
        environment: dict[str, str] | None = None,
    ) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object], Path]:
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        report_path = self.root / f"report-{token}.xml"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
                "-B",
                "-X",
                "utf8",
                "-u",
                "-m",
                "_kat_runtime",
                "--request",
                str(request_path),
                "--response",
                str(response_path),
                "--test-report",
                str(report_path),
            ],
            cwd=pack,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env={**os.environ, "NO_COLOR": "1", **(environment or {})},
        )
        if not response_path.exists():
            self.fail(
                "Runtime did not write a response:\n"
                + completed.stderr.decode(errors="replace")
            )
        return (
            completed,
            json.loads(response_path.read_text(encoding="utf-8")),
            report_path,
        )

    def pack(self) -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "helpers").mkdir()
        (pack / "tests" / "nested").mkdir(parents=True)
        (pack / "helpers" / "rules.py").write_text(
            "OFFSET = 1\n",
            encoding="utf-8",
        )
        (pack / "workflows" / "analyze.py").write_text(
            '''import kat
import pyarrow as pa
from kat.pack.helpers import rules

@kat.workflow(
    name="analyze",
    title="Analyze",
    parameters={"minimum": "Minimum"},
)
def analyze(ctx: kat.Context, *, minimum: int = 0):
    """Analyze generated values."""
    return ctx.from_arrow(pa.table({"value": [minimum + rules.OFFSET]}))
''',
            encoding="utf-8",
        )
        return pack.resolve()

    @staticmethod
    def request(
        pack: Path,
        *,
        datasets: dict[str, object] | None = None,
        tests: list[str] | None = None,
    ) -> dict[str, object]:
        return {
            "operation": "test_pack",
            "pack_name": "example",
            "pack_path": str(pack),
            "datasets": datasets or {},
            "tests": tests or [],
        }

    @staticmethod
    def write_test(pack: Path, name: str, source: str) -> None:
        (pack / "tests" / "nested" / name).write_text(source, encoding="utf-8")

    @staticmethod
    def replace_workflow(pack: Path, source: str) -> None:
        (pack / "workflows" / "analyze.py").write_text(source, encoding="utf-8")

    def assert_pack_tests_failed(
        self,
        completed: subprocess.CompletedProcess[bytes],
        response: dict[str, object],
        report: Path,
    ) -> str:
        self.assertEqual(
            completed.returncode,
            0,
            completed.stderr.decode(errors="replace"),
        )
        self.assertEqual(response["status"], "failure", response)
        self.assertEqual(response["error"]["message"], "PACK tests failed")
        self.assertNotIn("result", response)
        self.assertTrue(report.is_file())
        return completed.stderr.decode(errors="replace")

    def test_native_pytest_owns_plugins_fixtures_and_pack_imports(self) -> None:
        pack = self.pack()
        (pack / "helpers" / "pytest_plugin.py").write_text(
            '''import pytest

@pytest.fixture
def plugin_value():
    return 40
''',
            encoding="utf-8",
        )
        (pack / "conftest.py").write_text(
            '''pytest_plugins = ("kat.pack.helpers.pytest_plugin",)
''',
            encoding="utf-8",
        )
        (pack / "tests" / "nested" / "conftest.py").write_text(
            '''import pytest

@pytest.fixture
def nested_value():
    return 2
''',
            encoding="utf-8",
        )
        self.write_test(
            pack,
            "test_workflow.py",
            '''import pyarrow as pa
import pytest
from kat.pack.helpers import rules

@pytest.mark.parametrize("minimum", [0, 1])
def test_workflow(kat_run, monkeypatch, plugin_value, nested_value, minimum):
    monkeypatch.setattr(rules, "OFFSET", plugin_value + nested_value)
    first = kat_run(workflow="analyze", arguments=["--minimum", str(minimum)])
    second = kat_run(workflow="analyze", arguments=["--minimum", str(minimum + 1)])
    assert isinstance(first["main"], pa.Table)
    assert first["main"].to_pydict() == {"value": [42 + minimum]}
    assert second["main"].to_pydict() == {"value": [43 + minimum]}
''',
        )
        (pack / "pytest.ini").write_text(
            "[pytest]\naddopts = --collect-only\n",
            encoding="utf-8",
        )
        (self.root / "conftest.py").write_text(
            "raise RuntimeError('parent conftest must not load')\n",
            encoding="utf-8",
        )

        completed, response, report = self.run_runtime(
            self.request(pack),
            pack,
            environment={"PYTEST_ADDOPTS": "--collect-only"},
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode(errors="replace"))
        self.assertEqual(completed.stdout, b"")
        self.assertEqual(response, {"status": "success", "result": {"summary": {"passed": 2}}})
        self.assertTrue(report.is_file())
        self.assertIn("2 passed", completed.stderr.decode(errors="replace"))
        self.assertFalse((pack / ".pytest_cache").exists())

    def test_selector_and_resolved_dataset_reach_kat_run(self) -> None:
        pack = self.pack()
        dataset = pack / "tests" / "datasets" / "sample"
        tables = write_materialized_source(
            dataset,
            pack="example",
            source="facts",
            tables={"events": pa.table({"value": [3, 7]})},
        )
        self.replace_workflow(
            pack,
            '''import kat

@kat.workflow(
    name="analyze",
    title="Analyze",
    parameters={"minimum": "Minimum"},
)
def analyze(ctx: kat.Context, *, minimum: int = 0):
    """Analyze event values."""
    return ctx.sql(
        "SELECT value FROM example.facts.events WHERE value >= $minimum",
        minimum=minimum,
    )
''',
        )
        self.write_test(
            pack,
            "test_workflow.py",
            '''def test_selected(kat_run):
    result = kat_run(workflow="analyze", dataset="sample", arguments=["--minimum", "5"])
    assert result["main"].to_pydict() == {"value": [7]}

def test_not_selected():
    raise AssertionError("the raw node id was not preserved")
''',
        )
        selector = "tests/nested/test_workflow.py::test_selected"
        completed, response, report = self.run_runtime(
            self.request(
                pack,
                datasets={
                    "sample": materialized_dataset_request(
                        dataset,
                        pack="example",
                        source="facts",
                        tables=tables,
                    )
                },
                tests=[selector],
            ),
            pack,
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {"status": "success", "result": {"summary": {"passed": 1}}},
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(report.is_file())
        terminal = completed.stderr.decode(errors="replace")
        self.assertIn("1 passed", terminal)
        self.assertNotIn("raw node id", terminal)

    def test_private_test_request_rejects_unvalidated_cli_facts(self) -> None:
        request_path = self.root / "trusted-test-pack-request.json"
        request_path.write_text(
            json.dumps(
                {
                    "operation": "test_pack",
                    "pack_name": "example",
                    "pack_path": "PACK/../PACK",
                    "datasets": {
                        "sample": {
                            "path": "datasets/../sample",
                            "tables": {"not-a-table-name": "tables/../events.parquet"},
                        }
                    },
                    "tests": ["../outside/test_workflow.py"],
                    "ignored_by_runtime": {"not": "a protocol error"},
                }
            ),
            encoding="utf-8",
        )

        with self.assertRaises(RuntimeRequestError):
            read_request(request_path)

    def test_summary_counts_setup_skips_without_counting_lifecycle_passes(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_skip.py",
            '''import pytest

@pytest.fixture(autouse=True)
def skip_before_call():
    pytest.skip("fixture skips this test")

def test_never_calls():
    raise AssertionError("unreachable")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {"status": "success", "result": {"summary": {"skipped": 1}}},
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(report.is_file())
        self.assertIn("1 skipped", completed.stderr.decode(errors="replace"))

    def test_summary_counts_collection_skips(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_collection_skip.py",
            '''import pytest
pytest.skip("module is not available", allow_module_level=True)
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK contains no collected tests")
        self.assertNotIn("result", response)
        self.assertTrue(report.is_file())
        self.assertIn("1 skipped", completed.stderr.decode(errors="replace"))

    def test_pytest_failures_have_no_partial_summary_and_keep_reports(self) -> None:
        cases = {
            "assertion": (
                "def test_failure():\n    assert False, 'visible assertion'\n",
                "PACK tests failed",
                "visible assertion",
            ),
            "collection": (
                "def test_broken(:\n    pass\n",
                "PACK tests were interrupted",
                "SyntaxError",
            ),
        }
        for name, (source, message, terminal_fragment) in cases.items():
            with self.subTest(name=name):
                pack = self.pack()
                self.write_test(pack, "test_failure.py", source)
                completed, response, report = self.run_runtime(self.request(pack), pack)
                self.assertEqual(completed.returncode, 0)
                self.assertEqual(response["status"], "failure")
                self.assertEqual(response["error"]["message"], message)
                self.assertNotIn("result", response)
                self.assertTrue(report.is_file())
                self.assertIn(terminal_fragment, completed.stderr.decode(errors="replace"))

        empty = self.pack()
        completed, response, report = self.run_runtime(self.request(empty), empty)
        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK contains no collected tests")
        self.assertNotIn("result", response)
        self.assertTrue(report.is_file())

    def test_production_interface_is_validated_before_pytest(self) -> None:
        pack = self.pack()
        sentinel = pack / "pytest-started"
        self.replace_workflow(
            pack,
            "raise RuntimeError('invalid production interface')\n",
        )
        self.write_test(
            pack,
            "test_never.py",
            f"from pathlib import Path\nPath({str(sentinel)!r}).write_text('started')\n",
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK test Runtime failed")
        self.assertNotIn("result", response)
        self.assertFalse(sentinel.exists())
        self.assertFalse(report.exists())

    def test_unknown_dataset_fails_even_when_the_workflow_needs_no_tables(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_dataset.py",
            '''def test_unknown_dataset(kat_run):
    kat_run(workflow="analyze", dataset="missing")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("unknown Test Dataset 'missing'; available: none", terminal)
        self.assertNotIn("Traceback", terminal)

    def test_unknown_workflow_keeps_the_execution_cause_in_pytest_output(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_unknown_workflow.py",
            '''def test_unknown_workflow(kat_run):
    kat_run(workflow="missing")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("KAT Workflow test execution failed", terminal)
        self.assertIn("Workflow 'missing' was not found in the selected PACK", terminal)
        self.assertNotIn("WorkflowExecutionFailure", terminal)
        self.assertNotIn("Traceback", terminal)

    def test_unhashable_workflow_argument_keeps_pytest_type_error(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_invalid_workflow.py",
            '''def test_unhashable_workflow(kat_run):
    kat_run(workflow=[])
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("TypeError:", terminal)
        self.assertIn("unhashable type: 'list'", terminal)
        self.assertIn("kat_run(workflow=[])", terminal)
        self.assertIn("runtime/testing.py", terminal.replace("\\", "/"))
        self.assertNotIn("caused by: Workflow", terminal)

    def test_workflow_failures_keep_the_execution_cause_in_pytest_output(self) -> None:
        cases = {
            "system_exit": (
                'raise SystemExit("sentinel workflow exit")',
                "sentinel workflow exit",
            ),
            "exception": (
                'raise RuntimeError("sentinel workflow error")',
                "sentinel workflow error",
            ),
        }
        for name, (failure, expected) in cases.items():
            with self.subTest(name=name):
                pack = self.pack()
                self.replace_workflow(
                    pack,
                    f'''import kat

@kat.workflow(
    name="analyze",
    title="Analyze",
)
def analyze(ctx: kat.Context):
    """Stop the workflow with a known execution failure."""
    {failure}
''',
                )
                self.write_test(
                    pack,
                    "test_workflow_failure.py",
                    '''def test_workflow_failure(kat_run):
    kat_run(workflow="analyze")
''',
                )

                completed, response, report = self.run_runtime(
                    self.request(pack),
                    pack,
                )

                terminal = self.assert_pack_tests_failed(completed, response, report)
                self.assertIn("KAT Workflow test execution failed", terminal)
                self.assertIn(expected, terminal)
                self.assertNotIn("Traceback", terminal)

    def test_unexpected_harness_error_keeps_its_traceback(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_harness_error.py",
            '''import _kat_runtime.execution as execution

def test_unexpected_harness_error(kat_run, monkeypatch):
    def fail_logging(*args, **kwargs):
        raise RuntimeError("unexpected logging setup")
    monkeypatch.setattr(execution, "workflow_logging", fail_logging)
    kat_run(workflow="analyze")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("RuntimeError: unexpected logging setup", terminal)
        self.assertIn("test_harness_error.py", terminal)
        self.assertNotIn("KAT Workflow test execution failed", terminal)

    def test_output_task_error_keeps_the_execution_cause_in_pytest_output(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_output_task_error.py",
            '''import _kat_runtime.outputs as outputs

async def fail_output(*args, **kwargs):
    raise RuntimeError("sentinel output task error")

def test_output_task_error(kat_run, monkeypatch):
    monkeypatch.setattr(outputs, "_write_output", fail_output)
    kat_run(workflow="analyze")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("KAT Workflow test execution failed", terminal)
        self.assertIn("sentinel output task error", terminal)
        self.assertNotIn("Traceback", terminal)

    def test_unexpected_eager_output_error_keeps_its_traceback(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_internal_error.py",
            '''import _kat_runtime.testing as testing

def test_unexpected_output_error(kat_run, monkeypatch):
    def fail_read(*args, **kwargs):
        raise RuntimeError("unexpected eager read")
    monkeypatch.setattr(testing.pq, "read_table", fail_read)
    kat_run(workflow="analyze")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("RuntimeError: unexpected eager read", terminal)
        self.assertIn("test_internal_error.py", terminal)


if __name__ == "__main__":
    unittest.main()
