from __future__ import annotations

from collections.abc import Callable
import concurrent.futures
from pathlib import Path
import tempfile
import threading
import time
import unittest
import uuid

import pyarrow as pa
import pyarrow.parquet as pq

import kat
from _kat_runtime.datasource import WorkflowOperation
from _kat_runtime.execution import (
    RunWorkflowRuntimeResult,
    WorkflowContext,
    WorkflowExecutionFailure,
    _ContextState,
    _NestedRunExecutor,
    run_loaded_workflow,
)
from _kat_runtime.inspection import compile_declared_workflow
from _kat_runtime.request import RunCandidateRef


class _BlockingNestedRuns:
    def __init__(self, relation: Path) -> None:
        self.relation = relation
        self.started = threading.Event()
        self.release = threading.Event()
        self.calls: list[tuple[str, str, dict[str, object]]] = []

    def run(
        self, pack_name: str, workflow_name: str, inputs: dict[str, object]
    ) -> dict[str, Path]:
        self.calls.append((pack_name, workflow_name, inputs))
        self.started.set()
        if not self.release.wait(timeout=5):
            raise RuntimeError("test did not release nested call")
        return {"main": self.relation}


class _BarrierNestedRuns:
    def __init__(self, relation: Path) -> None:
        self.relation = relation
        self.entered = threading.Barrier(2)
        self.release = threading.Barrier(2)
        self.calls: list[tuple[str, str, dict[str, object]]] = []

    def run(
        self, pack_name: str, workflow_name: str, inputs: dict[str, object]
    ) -> dict[str, Path]:
        self.calls.append((pack_name, workflow_name, inputs))
        self.entered.wait(timeout=5)
        self.release.wait(timeout=5)
        return {"main": self.relation}


class _FailingNestedRuns:
    def run(
        self, pack_name: str, workflow_name: str, inputs: dict[str, object]
    ) -> dict[str, Path]:
        del pack_name, workflow_name, inputs
        raise kat.RunError("sentinel nested Workflow failure")


_ACTIVE_NESTED_RUNS: _BlockingNestedRuns | None = None
_ACTIVE_WORKER_THREADS: list[threading.Thread] = []


@kat.workflow(name="parent-active", description="Start one unjoined child.")
def _parent_with_active_call(ctx: kat.Context):
    nested_runs = _ACTIVE_NESTED_RUNS
    if nested_runs is None:
        raise RuntimeError("test nested-run client is not configured")
    thread = threading.Thread(
        target=ctx.run,
        args=("child-pack", "analyze"),
    )
    _ACTIVE_WORKER_THREADS.append(thread)
    thread.start()
    if not nested_runs.started.wait(timeout=5):
        raise RuntimeError("nested call did not start")
    return kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))


@kat.workflow(name="parent-join", description="Join a child thread without rethrowing.")
def _parent_with_plain_join(ctx: kat.Context):
    """Exercise ordinary Thread.join exception semantics."""
    thread = threading.Thread(
        target=ctx.run,
        args=("child-pack", "analyze"),
    )
    thread.start()
    thread.join()
    return kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))


@kat.workflow(name="parent-future", description="Collect a child Future result.")
def _parent_with_future_result(ctx: kat.Context):
    """Exercise ordinary Future.result exception semantics."""
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        pool.submit(ctx.run, "child-pack", "analyze").result()
    return kat.dataprovider.Table.from_arrow(pa.table({"value": [1]}))


class NestedWorkflowContextTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.datasource_root = self.root / "materializations"
        self.scratch_root = self.root / "scratch"
        self.datasource_root.mkdir()
        self.scratch_root.mkdir()
        self.relation = self.root / "main.parquet"
        pq.write_table(pa.table({"value": [7]}), self.relation)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _run_declared_workflow(
        self,
        function: Callable[..., object],
        nested_runs: _NestedRunExecutor,
    ) -> RunWorkflowRuntimeResult:
        candidate_id = str(uuid.uuid7())
        candidate = self.root / "runs" / candidate_id
        scratch = self.root / "run-scratch" / candidate_id
        candidate.mkdir(parents=True)
        scratch.mkdir(parents=True)
        workflow = compile_declared_workflow(function)
        return run_loaded_workflow(
            workflow,
            pack_name="parent-pack",
            workflow_name=workflow.interface["name"],
            arguments=[],
            candidate=RunCandidateRef(candidate_id, candidate),
            datasource_root=self.datasource_root,
            scratch_root=scratch,
            nested_runs=nested_runs,
        )

    def test_close_collects_registered_calls_and_rejects_late_calls(self) -> None:
        nested_runs = _BarrierNestedRuns(self.relation)
        context = WorkflowContext(
            WorkflowOperation(self.datasource_root, self.scratch_root),
            nested_runs,
        )

        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            call = pool.submit(context.run, "child-pack", "analyze", value=7)
            nested_runs.entered.wait(timeout=5)
            first_close = pool.submit(context.close)
            second_close = pool.submit(context.close)
            deadline = time.monotonic() + 5
            while True:
                with context._condition:
                    if context._state is _ContextState.CLOSING:
                        break
                if time.monotonic() >= deadline:
                    self.fail("Workflow Context did not begin closing")
                time.sleep(0.001)
            with self.assertRaisesRegex(kat.RunError, "closed"):
                context.run("child-pack", "late")
            self.assertFalse(first_close.done())
            self.assertFalse(second_close.done())
            nested_runs.release.wait(timeout=5)
            catalog = call.result(timeout=5)
            self.assertTrue(first_close.result(timeout=5))
            self.assertTrue(second_close.result(timeout=5))

        self.assertEqual(catalog.tables, ("main",))
        self.assertTrue(context.close())
        selected = kat.dataprovider.DataFusionProvider(catalog=catalog).query(
            "SELECT value FROM main"
        )
        self.assertEqual(selected["value"], (7,))
        self.assertEqual(
            nested_runs.calls,
            [("child-pack", "analyze", {"value": 7})],
        )

    def test_catalog_construction_failure_is_a_sanitized_run_error(self) -> None:
        missing = self.root / "private-missing-output.parquet"
        nested_runs = _BlockingNestedRuns(missing)
        nested_runs.release.set()
        context = WorkflowContext(
            WorkflowOperation(self.datasource_root, self.scratch_root),
            nested_runs,
        )

        with self.assertLogs("_kat_runtime.execution", level="ERROR"):
            with self.assertRaises(kat.RunError) as raised:
                context.run("child-pack", "analyze")

        self.assertNotIn(str(missing), str(raised.exception))
        self.assertIsNone(raised.exception.__cause__)
        self.assertFalse(context.close())

    def test_future_result_rethrows_but_plain_thread_join_does_not(self) -> None:
        nested_runs = _FailingNestedRuns()
        uncaught: list[BaseException] = []
        previous_excepthook = threading.excepthook
        threading.excepthook = lambda arguments: uncaught.append(arguments.exc_value)
        try:
            joined = self._run_declared_workflow(_parent_with_plain_join, nested_runs)
        finally:
            threading.excepthook = previous_excepthook

        self.assertEqual(tuple(joined.outputs), ("main",))
        self.assertEqual(len(uncaught), 1)
        self.assertIsInstance(uncaught[0], kat.RunError)
        with self.assertRaises(WorkflowExecutionFailure) as raised:
            self._run_declared_workflow(_parent_with_future_result, nested_runs)
        self.assertIsInstance(raised.exception.__cause__, kat.RunError)
        self.assertIn("sentinel nested Workflow failure", str(raised.exception.__cause__))

    def test_parent_return_with_an_active_call_waits_and_fails_before_output(self) -> None:
        global _ACTIVE_NESTED_RUNS
        nested_runs = _BlockingNestedRuns(self.relation)
        _ACTIVE_NESTED_RUNS = nested_runs
        _ACTIVE_WORKER_THREADS.clear()

        candidate_id = str(uuid.uuid7())
        candidate = self.root / "runs" / candidate_id
        scratch = self.root / "run-scratch" / candidate_id
        candidate.mkdir(parents=True)
        scratch.mkdir(parents=True)

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            execution = pool.submit(
                run_loaded_workflow,
                compile_declared_workflow(_parent_with_active_call),
                pack_name="parent-pack",
                workflow_name="parent-active",
                arguments=[],
                candidate=RunCandidateRef(candidate_id, candidate),
                datasource_root=self.datasource_root,
                scratch_root=scratch,
                nested_runs=nested_runs,
            )
            self.assertTrue(nested_runs.started.wait(timeout=5))
            self.assertFalse(execution.done())
            nested_runs.release.set()
            with self.assertRaises(WorkflowExecutionFailure) as raised:
                execution.result(timeout=5)

        _ACTIVE_NESTED_RUNS = None
        for thread in _ACTIVE_WORKER_THREADS:
            thread.join(timeout=5)
        self.assertIsInstance(raised.exception.__cause__, kat.RunError)
        self.assertIn("still running", str(raised.exception.__cause__))
        self.assertFalse((candidate / "outputs").exists())


if __name__ == "__main__":
    unittest.main()
