from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
from enum import Enum, auto
import logging
from pathlib import Path
import sys
import threading
from typing import Iterator, Protocol

import kat

from kat._temporal import _duration_nanoseconds

from .datasource import WorkflowOperation
from .inspection import CompiledWorkflow
from .outputs import materialize_outputs
from .pack import ProductionPack
from .request import RunCandidateRef, RunWorkflowRequest


@dataclass(frozen=True)
class RunWorkflowRuntimeResult:
    effective_inputs: dict[str, object]
    outputs: dict[str, dict[str, object]]


class WorkflowExecutionFailure(Exception):
    """已知且可由 PACK 作者纠正的 Workflow 解析或执行路径失败。

    包括已加载 Workflow 的参数、用户函数与 Output materialization；kat_run 的
    Workflow 名称查找未命中也使用此类。非法 Python
    fixture 实参及 pytest plugin、fixture、日志设施等 harness 异常不属于此类。
    """


class _NestedRunExecutor(Protocol):
    def run(
        self,
        pack_name: str,
        workflow_name: str,
        inputs: dict[str, object],
    ) -> dict[str, Path]: ...


class _ContextState(Enum):
    OPEN = auto()
    CLOSING = auto()
    CLOSED = auto()


class WorkflowContext(kat.Context):
    def __init__(
        self,
        operation: WorkflowOperation,
        nested_runs: _NestedRunExecutor | None = None,
    ) -> None:
        self._operation = operation
        self._nested_runs = nested_runs
        self._condition = threading.Condition()
        self._state = _ContextState.OPEN
        self._active_calls = 0
        self._had_active_calls_on_close: bool | None = None

    @property
    def datasource_root(self) -> Path:
        return self._operation.datasource_root

    @property
    def scratch_root(self) -> Path:
        return self._operation.scratch_root

    def run(
        self,
        pack_name: str,
        workflow_name: str,
        /,
        **inputs: object,
    ) -> kat.dataprovider.Catalog:
        with self._condition:
            if self._state is not _ContextState.OPEN:
                raise kat.RunError("Workflow Context is closed")
            self._active_calls += 1
        try:
            if self._nested_runs is None:
                raise kat.RunError("Nested Workflow execution is unavailable")
            try:
                relations = self._nested_runs.run(
                    pack_name,
                    workflow_name,
                    dict(inputs),
                )
            except kat.RunError:
                raise
            except (Exception, SystemExit):
                logging.getLogger(__name__).exception(
                    "unexpected nested Workflow client failure"
                )
                raise kat.RunError("Nested Workflow execution failed") from None
            try:
                return kat.dataprovider.open(tables=relations)
            except (Exception, SystemExit):
                logging.getLogger(__name__).exception(
                    "failed to construct the nested Workflow Output Catalog"
                )
                raise kat.RunError(
                    "Nested Workflow Output Catalog is unavailable"
                ) from None
        finally:
            with self._condition:
                self._active_calls -= 1
                self._condition.notify_all()

    def close(self) -> bool:
        """Stop accepting calls, collect registered calls, and return whether any were active."""
        with self._condition:
            if self._state is _ContextState.CLOSED:
                assert self._had_active_calls_on_close is not None
                return self._had_active_calls_on_close
            if self._state is not _ContextState.OPEN:
                while self._state is _ContextState.CLOSING:
                    self._condition.wait()
                assert self._had_active_calls_on_close is not None
                return self._had_active_calls_on_close
            self._state = _ContextState.CLOSING
            self._had_active_calls_on_close = self._active_calls != 0
            while self._active_calls:
                self._condition.wait()
            self._state = _ContextState.CLOSED
            self._condition.notify_all()
            return self._had_active_calls_on_close


def run_workflow(
    request: RunWorkflowRequest,
    nested_runs: _NestedRunExecutor | None = None,
) -> RunWorkflowRuntimeResult:
    pack_name = request.pack_name
    workflow_name = request.workflow_name
    with _workflow_operation(
        request.datasource_root, request.scratch_root
    ) as operation:
        workflow = ProductionPack.open(pack_name, request.pack_path).load(workflow_name)
        return _run_loaded_workflow(
            workflow,
            operation=operation,
            pack_name=pack_name,
            workflow_name=workflow_name,
            arguments=request.arguments,
            inputs=request.inputs,
            candidate=request.candidate,
            nested_runs=nested_runs,
        )


def run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    pack_name: str,
    workflow_name: str,
    arguments: list[str] | None,
    inputs: dict[str, object] | None = None,
    candidate: RunCandidateRef,
    datasource_root: Path,
    scratch_root: Path,
    nested_runs: _NestedRunExecutor | None = None,
) -> RunWorkflowRuntimeResult:
    with _workflow_operation(datasource_root, scratch_root) as operation:
        return _run_loaded_workflow(
            workflow,
            operation=operation,
            pack_name=pack_name,
            workflow_name=workflow_name,
            arguments=arguments,
            inputs=inputs,
            candidate=candidate,
            nested_runs=nested_runs,
        )


def _run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    operation: WorkflowOperation,
    pack_name: str,
    workflow_name: str,
    arguments: list[str] | None,
    inputs: dict[str, object] | None,
    candidate: RunCandidateRef,
    nested_runs: _NestedRunExecutor | None,
) -> RunWorkflowRuntimeResult:
    candidate_id = candidate.identifier
    candidate_path = candidate.path

    try:
        if (arguments is None) == (inputs is None):
            raise ValueError("Workflow execution requires exactly one input representation")
        if arguments is not None:
            effective = workflow.parse_arguments(arguments)
        else:
            assert inputs is not None
            effective = workflow.parse_inputs(inputs)
    except ValueError as error:
        raise WorkflowExecutionFailure() from error
    context = WorkflowContext(operation, nested_runs)
    with workflow_logging(candidate_id, pack_name, workflow_name):
        try:
            try:
                value = workflow.function(context, **effective)
            except (Exception, SystemExit) as error:
                raise WorkflowExecutionFailure() from error
        finally:
            had_active_calls = context.close()
        if had_active_calls:
            raise WorkflowExecutionFailure() from kat.RunError(
                "Workflow returned while nested Workflow calls were still running"
            )
        try:
            outputs = materialize_outputs(value, candidate_path)
        except (Exception, SystemExit) as error:
            raise WorkflowExecutionFailure() from error
    return RunWorkflowRuntimeResult(
        effective_inputs={
            name: _project_effective_input(value) for name, value in effective.items()
        },
        outputs=outputs,
    )


@contextmanager
def _workflow_operation(
    datasource_root: Path, scratch_root: Path
) -> Iterator[WorkflowOperation]:
    operation = WorkflowOperation(datasource_root, scratch_root)
    try:
        yield operation
    except BaseException as execution_error:
        operation.expire()
        try:
            operation.cleanup_scratch()
        except BaseException as cleanup_error:
            _append_cleanup_error(execution_error, cleanup_error)
        raise
    else:
        operation.expire()
        operation.cleanup_scratch()


def _append_cleanup_error(
    execution_error: BaseException, cleanup_error: BaseException
) -> None:
    # 清理是发布门，但不是已经发生的执行失败的根因；把它追加到现有异常链末端。
    seen: set[int] = set()
    current = execution_error
    while id(current) not in seen:
        seen.add(id(current))
        cause = BaseException.__cause__.__get__(current, BaseException)
        if cause is not None:
            next_error = cause
        elif BaseException.__suppress_context__.__get__(current, BaseException):
            break
        else:
            next_error = BaseException.__context__.__get__(current, BaseException)
            if next_error is None:
                break
        if id(next_error) in seen:
            break
        current = next_error
    current.__cause__ = cleanup_error
    current.__suppress_context__ = True


def _project_effective_input(value: object) -> object:
    if value is None or type(value) in (str, bool, float):
        return value
    if type(value) is int:
        return str(value)
    if isinstance(value, kat.Duration):
        return str(_duration_nanoseconds(str(value)))
    if isinstance(value, kat.WallClockTimestamp):
        return str(value)
    raise TypeError("Workflow Input Compiler produced an unsupported effective value")


@contextmanager
def workflow_logging(
    candidate_id: str, pack_name: str, workflow_name: str
) -> Iterator[None]:
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(
        logging.Formatter(
            f"%(levelname)s candidate={candidate_id} pack={pack_name} "
            f"workflow={workflow_name} %(name)s: %(message)s"
        )
    )
    root = logging.getLogger()
    previous_level = root.level
    root.addHandler(handler)
    if previous_level > logging.INFO:
        root.setLevel(logging.INFO)
    try:
        yield
    finally:
        root.removeHandler(handler)
        root.setLevel(previous_level)
