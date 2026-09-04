from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
import logging
from pathlib import Path
import sys
from typing import Iterator

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


class WorkflowContext(kat.Context):
    def __init__(self, operation: WorkflowOperation) -> None:
        self._operation = operation

    @property
    def datasource_root(self) -> Path:
        return self._operation.datasource_root

    @property
    def scratch_root(self) -> Path:
        return self._operation.scratch_root


def run_workflow(request: RunWorkflowRequest) -> RunWorkflowRuntimeResult:
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
            candidate=request.candidate,
        )


def run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    pack_name: str,
    workflow_name: str,
    arguments: list[str],
    candidate: RunCandidateRef,
    datasource_root: Path,
    scratch_root: Path,
) -> RunWorkflowRuntimeResult:
    with _workflow_operation(datasource_root, scratch_root) as operation:
        return _run_loaded_workflow(
            workflow,
            operation=operation,
            pack_name=pack_name,
            workflow_name=workflow_name,
            arguments=arguments,
            candidate=candidate,
        )


def _run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    operation: WorkflowOperation,
    pack_name: str,
    workflow_name: str,
    arguments: list[str],
    candidate: RunCandidateRef,
) -> RunWorkflowRuntimeResult:
    candidate_id = candidate.identifier
    candidate_path = candidate.path

    try:
        effective = workflow.parse_arguments(arguments)
    except ValueError as error:
        raise WorkflowExecutionFailure() from error
    context = WorkflowContext(operation)
    with workflow_logging(candidate_id, pack_name, workflow_name):
        try:
            value = workflow.function(context, **effective)
        except (Exception, SystemExit) as error:
            raise WorkflowExecutionFailure() from error
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
