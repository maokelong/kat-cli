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

from _kat_runtime.request import TestPackRequest as _TestPackRequest
from _kat_runtime.request import RunWorkflowRequest as _RunWorkflowRequest
from _kat_runtime.request import RuntimeRequestError, read_request
from _test_control_peer import run_runtime_with_test_control


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
        arguments = [
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
        ]
        completed = run_runtime_with_test_control(
            arguments,
            cwd=pack,
            environment={
                **os.environ,
                "NO_COLOR": "1",
                **(environment or {}),
            },
            data_home=self.root / f"host-{token}",
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
from kat import dataprovider as dp
from kat.pack.helpers import rules

@kat.workflow(
    name="analyze",
    description="Analyze generated values.",
    parameters={"minimum": "Minimum"},
)
def analyze(ctx: kat.Context, *, minimum: int = 0):
    """Analyze generated values."""
    return dp.Table.from_arrow(pa.table({"value": [minimum + rules.OFFSET]}))
''',
            encoding="utf-8",
        )
        return pack.resolve()

    @staticmethod
    def request(
        pack: Path,
        *,
        tests: list[str] | None = None,
    ) -> dict[str, object]:
        return {
            "operation": "test_pack",
            "pack_name": "example",
            "pack_path": str(pack),
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

    def test_test_pack_request_has_an_exact_dataset_free_shape(self) -> None:
        request_path = self.root / "test-pack-request.json"
        request_path.write_text(
            json.dumps(
                {
                    "operation": "test_pack",
                    "pack_name": "example",
                    "pack_path": "PACK/../PACK",
                    "tests": ["tests/test_workflow.py"],
                }
            ),
            encoding="utf-8",
        )
        request = read_request(request_path)

        self.assertIsInstance(request, _TestPackRequest)
        self.assertEqual(request.pack_name, "example")
        self.assertEqual(request.pack_path, Path("PACK/../PACK"))
        self.assertEqual(request.tests, ["tests/test_workflow.py"])
        self.assertFalse(hasattr(request, "datasets"))

        legacy_request = json.loads(request_path.read_text(encoding="utf-8"))
        legacy_request["datasets"] = {}
        request_path.write_text(json.dumps(legacy_request), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeRequestError, "fields must be exactly"):
            read_request(request_path)

    def test_run_workflow_request_has_an_exact_session_root_shape(self) -> None:
        pack = self.root / "request-pack"
        pack.mkdir()
        data_home = self.root / "data-home"
        session_id = f"019f0000-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        session = data_home / "sessions" / session_id
        for name in ("materializations", "scratch", "runs"):
            (session / name).mkdir(parents=True)
        candidate_id = f"019f0000-0000-7000-8000-{uuid.uuid4().hex[:12]}"
        candidate_path = session / "runs" / candidate_id
        candidate_path.mkdir()
        scratch_root = session / "scratch" / candidate_id
        scratch_root.mkdir()
        request_path = self.root / "run-workflow-request.json"
        request_path.write_text(
            json.dumps(
                {
                    "operation": "run_workflow",
                    "pack_name": "example",
                    "pack_path": str(pack.resolve()),
                    "workflow_name": "analyze",
                    "arguments": [],
                    "candidate_id": candidate_id,
                    "candidate_path": str(candidate_path.resolve()),
                    "datasource_root": str((session / "materializations").resolve()),
                    "scratch_root": str(scratch_root.resolve()),
                }
            ),
            encoding="utf-8",
        )
        self.assertEqual(
            set(json.loads(request_path.read_text(encoding="utf-8"))),
            {
                "operation",
                "pack_name",
                "pack_path",
                "workflow_name",
                "arguments",
                "candidate_id",
                "candidate_path",
                "datasource_root",
                "scratch_root",
            },
        )

        request = read_request(request_path)

        self.assertIsInstance(request, _RunWorkflowRequest)
        self.assertEqual(request.workflow_name, "analyze")
        self.assertEqual(request.arguments, [])
        self.assertIsNone(request.inputs)
        self.assertEqual(request.datasource_root, (session / "materializations").resolve())
        self.assertEqual(request.scratch_root, scratch_root.resolve())
        self.assertFalse(hasattr(request, "dataset"))

        legacy_request = json.loads(request_path.read_text(encoding="utf-8"))
        legacy_request["dataset"] = {}
        request_path.write_text(json.dumps(legacy_request), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeRequestError, "invalid field set"):
            read_request(request_path)

        nested_request = json.loads(request_path.read_text(encoding="utf-8"))
        nested_request.pop("dataset")
        nested_request["operation"] = "run_workflow_with_inputs"
        nested_request.pop("arguments")
        nested_request["inputs"] = {
            "minimum": {"type": "int64", "value": "2"},
            "enabled": {"type": "boolean", "value": True},
        }
        self.assertEqual(
            set(nested_request),
            {
                "operation",
                "pack_name",
                "pack_path",
                "workflow_name",
                "inputs",
                "candidate_id",
                "candidate_path",
                "datasource_root",
                "scratch_root",
            },
        )
        request_path.write_text(json.dumps(nested_request), encoding="utf-8")
        nested = read_request(request_path)
        self.assertIsNone(nested.arguments)
        self.assertEqual(nested.inputs, {"minimum": 2, "enabled": True})

        for invalid in (
            {**nested_request, "arguments": []},
            {
                **nested_request,
                "operation": "run_workflow",
                "arguments": [],
            },
        ):
            request_path.write_text(json.dumps(invalid), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeRequestError, "invalid field set"):
                read_request(request_path)

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

    def test_raw_pytest_selector_reaches_kat_run(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_workflow.py",
            '''def test_selected(kat_run):
    result = kat_run(workflow="analyze", arguments=["--minimum", "5"])
    assert result["main"].to_pydict() == {"value": [6]}

def test_not_selected():
    raise AssertionError("the raw node id was not preserved")
''',
        )
        selector = "tests/nested/test_workflow.py::test_selected"
        completed, response, report = self.run_runtime(
            self.request(pack, tests=[selector]),
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

    def test_pack_owned_datasource_provider_is_an_ordinary_python_class(self) -> None:
        pack = self.pack()
        datasources = pack / "datasources"
        datasources.mkdir()
        (datasources / "__init__.py").write_text(
            "DEFAULT_VALUE = 1\n",
            encoding="utf-8",
        )
        (datasources / "provider_state.py").write_text(
            '''import pyarrow as pa
from kat import dataprovider as dp

from . import DEFAULT_VALUE


providers = []


class Provider:
    def __init__(self):
        self.query_count = 0

    def query(self, value=DEFAULT_VALUE):
        self.query_count += 1
        return dp.Table.from_arrow(pa.table({"value": [value]}))


def create():
    provider = Provider()
    providers.append(provider)
    return provider
''',
            encoding="utf-8",
        )
        self.replace_workflow(
            pack,
            '''import kat
from kat.pack.datasources import provider_state


@kat.workflow(name="analyze", description="Publish one PACK-owned Provider result.")
def analyze(ctx: kat.Context):
    """Publish one PACK-owned Provider result."""
    return provider_state.create().query()
''',
        )
        self.write_test(
            pack,
            "test_provider_lifecycle.py",
            '''from kat import dataprovider as dp
from kat.pack.datasources import provider_state


def test_provider_is_not_bound_to_a_workflow_lease(kat_run):
    result = kat_run(workflow="analyze")
    assert result["main"].to_pydict() == {"value": [1]}
    provider = provider_state.providers[-1]
    assert provider.query_count == 1

    later = provider.query(2)
    assert isinstance(later, dp.Table)
    assert later["value"] == (2,)
    assert provider.query_count == 2
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {"status": "success", "result": {"summary": {"passed": 1}}},
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(report.is_file())

    def test_kat_run_uses_one_session_per_test_and_one_scratch_per_call(
        self,
    ) -> None:
        pack = self.pack()
        (pack / "helpers" / "datasource_state.py").write_text(
            "datasource_roots = []\nscratch_roots = []\ncontexts = []\n",
            encoding="utf-8",
        )
        self.replace_workflow(
            pack,
            '''import kat
import pyarrow as pa
from kat import dataprovider as dp
from kat.pack.helpers import datasource_state


@kat.workflow(name="analyze", description="Increment a test-scoped Datasource materialization.")
def analyze(ctx: kat.Context):
    """Increment a test-scoped Datasource materialization."""
    root = ctx.datasource_root
    scratch = ctx.scratch_root
    counter = root / "counter.txt"
    value = int(counter.read_text(encoding="utf-8")) + 1 if counter.exists() else 1
    counter.write_text(str(value), encoding="utf-8")
    scratch.joinpath("temporary.txt").write_text(str(value), encoding="utf-8")
    datasource_state.datasource_roots.append(root)
    datasource_state.scratch_roots.append(scratch)
    datasource_state.contexts.append(ctx)
    return dp.Table.from_arrow(pa.table({"value": [value]}))
''',
        )
        self.write_test(
            pack,
            "test_datasource_root.py",
            '''import pytest
import uuid
from kat.pack.helpers import datasource_state


def test_shared_within_one_test(kat_run):
    first = kat_run(workflow="analyze")
    second = kat_run(workflow="analyze")
    assert first["main"].to_pydict() == {"value": [1]}
    assert second["main"].to_pydict() == {"value": [2]}
    first_root, second_root = datasource_state.datasource_roots[-2:]
    first_scratch, second_scratch = datasource_state.scratch_roots[-2:]
    assert first_root == second_root
    assert first_root.name == "materializations"
    assert first_scratch != second_scratch
    assert first_scratch.name != second_scratch.name
    assert uuid.UUID(first_scratch.name).version == 7
    assert uuid.UUID(second_scratch.name).version == 7
    assert first_scratch.parent.parent == first_root.parent
    assert second_scratch.parent.parent == second_root.parent
    assert not first_scratch.exists()
    assert not second_scratch.exists()
    for context in datasource_state.contexts[-2:]:
        with pytest.raises(RuntimeError, match="lease is no longer active"):
            _ = context.datasource_root
        with pytest.raises(RuntimeError, match="lease is no longer active"):
            _ = context.scratch_root


def test_isolated_from_the_previous_test(kat_run):
    previous_session = datasource_state.datasource_roots[0].parent
    result = kat_run(workflow="analyze")
    assert result["main"].to_pydict() == {"value": [1]}
    current_root = datasource_state.datasource_roots[-1]
    current_scratch = datasource_state.scratch_roots[-1]
    assert current_root.parent != previous_session
    assert current_scratch.parent.parent == current_root.parent
    assert not current_scratch.exists()
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        self.assertEqual(completed.returncode, 0)
        self.assertEqual(
            response,
            {"status": "success", "result": {"summary": {"passed": 2}}},
            completed.stderr.decode(errors="replace"),
        )
        self.assertTrue(report.is_file())

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

    def test_kat_run_rejects_the_removed_dataset_selector(self) -> None:
        pack = self.pack()
        self.write_test(
            pack,
            "test_dataset.py",
            '''def test_removed_dataset_selector(kat_run):
    kat_run(workflow="analyze", dataset="missing")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("TypeError:", terminal)
        self.assertIn("unexpected keyword argument 'dataset'", terminal)

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
        self.assertIn("_kat_runtime/testing.py", terminal.replace("\\", "/"))
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
    description="Stop the workflow with a known execution failure.",
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

    def test_workflow_failure_hides_a_suppressed_execution_context(self) -> None:
        pack = self.pack()
        self.replace_workflow(
            pack,
            '''import kat

@kat.workflow(
    name="analyze",
    description="Translate a Data Provider failure without exposing its private context.",
)
def analyze(ctx: kat.Context):
    """Translate a Data Provider failure without exposing its private context."""
    try:
        raise RuntimeError("PGPASSWORD=private-sentinel")
    except RuntimeError:
        raise RuntimeError("PostgreSQL query failed") from None
''',
        )
        self.write_test(
            pack,
            "test_workflow_failure.py",
            '''def test_workflow_failure(kat_run):
    kat_run(workflow="analyze")
''',
        )

        completed, response, report = self.run_runtime(self.request(pack), pack)

        terminal = self.assert_pack_tests_failed(completed, response, report)
        self.assertIn("KAT Workflow test execution failed", terminal)
        self.assertIn("PostgreSQL query failed", terminal)
        self.assertNotIn("private-sentinel", terminal)
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
