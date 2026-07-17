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
import pyarrow.parquet as pq


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
    required_tables=[],
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
        (pack / "tests" / "nested" / "test_workflow.py").write_text(
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
            encoding="utf-8",
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
        self.assertEqual(response, {"status": "success", "result": {"summary": {"passed": 6}}})
        self.assertTrue(report.is_file())
        self.assertIn("2 passed", completed.stderr.decode(errors="replace"))
        self.assertFalse((pack / ".pytest_cache").exists())

    def test_selector_and_resolved_dataset_reach_kat_run(self) -> None:
        pack = self.pack()
        dataset = pack / "tests" / "datasets" / "sample"
        dataset.mkdir(parents=True)
        events = dataset / "events.parquet"
        pq.write_table(pa.table({"value": [3, 7]}), events)
        (pack / "workflows" / "analyze.py").write_text(
            '''import kat

@kat.workflow(
    name="analyze",
    title="Analyze",
    required_tables=["events"],
    parameters={"minimum": "Minimum"},
)
def analyze(ctx: kat.Context, *, minimum: int = 0):
    """Analyze event values."""
    return ctx.sql("SELECT value FROM events WHERE value >= $minimum", minimum=minimum)
''',
            encoding="utf-8",
        )
        test_path = pack / "tests" / "nested" / "test_workflow.py"
        test_path.write_text(
            '''def test_selected(kat_run):
    result = kat_run(workflow="analyze", dataset="sample", arguments=["--minimum", "5"])
    assert result["main"].to_pydict() == {"value": [7]}

def test_not_selected():
    raise AssertionError("the raw node id was not preserved")
''',
            encoding="utf-8",
        )
        selector = "tests/nested/test_workflow.py::test_selected"
        completed, response, report = self.run_runtime(
            self.request(
                pack,
                datasets={
                    "sample": {
                        "path": str(dataset.resolve()),
                        "tables": {"events": str(events.resolve())},
                    }
                },
                tests=[selector],
            ),
            pack,
        )

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {"status": "success", "result": {"summary": {"passed": 2}}},
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(report.is_file())
        terminal = completed.stderr.decode(errors="replace")
        self.assertIn("1 passed", terminal)
        self.assertNotIn("raw node id", terminal)

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
                (pack / "tests" / "nested" / "test_failure.py").write_text(
                    source,
                    encoding="utf-8",
                )
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
        (pack / "workflows" / "analyze.py").write_text(
            "raise RuntimeError('invalid production interface')\n",
            encoding="utf-8",
        )
        (pack / "tests" / "nested" / "test_never.py").write_text(
            f"from pathlib import Path\nPath({str(sentinel)!r}).write_text('started')\n",
            encoding="utf-8",
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
        (pack / "tests" / "nested" / "test_dataset.py").write_text(
            '''def test_unknown_dataset(kat_run):
    kat_run(workflow="analyze", dataset="missing")
''',
            encoding="utf-8",
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK tests failed")
        self.assertNotIn("result", response)
        self.assertTrue(report.is_file())
        terminal = completed.stderr.decode(errors="replace")
        self.assertIn("unknown Test Dataset 'missing'; available: none", terminal)
        self.assertNotIn("Traceback", terminal)

    def test_unexpected_eager_output_error_keeps_its_traceback(self) -> None:
        pack = self.pack()
        (pack / "tests" / "nested" / "test_internal_error.py").write_text(
            '''import _kat_runtime.execution as execution

def test_unexpected_output_error(kat_run, monkeypatch):
    def fail_read(*args, **kwargs):
        raise RuntimeError("unexpected eager read")
    monkeypatch.setattr(execution.pq, "read_table", fail_read)
    kat_run(workflow="analyze")
''',
            encoding="utf-8",
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(response["status"], "failure")
        self.assertEqual(response["error"]["message"], "PACK tests failed")
        self.assertTrue(report.is_file())
        terminal = completed.stderr.decode(errors="replace")
        self.assertIn("RuntimeError: unexpected eager read", terminal)
        self.assertIn("test_internal_error.py", terminal)


if __name__ == "__main__":
    unittest.main()
