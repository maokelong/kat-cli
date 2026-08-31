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


def run_workflow(request: RunWorkflowRequest) -> RunWorkflowRuntimeResult:
    pack_name = request.pack_name
    workflow_name = request.workflow_name
    workflow = ProductionPack.open(pack_name, request.pack_path).load(workflow_name)
    return run_loaded_workflow(
        workflow,
        pack_name=pack_name,
        workflow_name=workflow_name,
        arguments=request.arguments,
        candidate=request.candidate,
        datasource_root=request.datasource_root,
    )


def run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    pack_name: str,
    workflow_name: str,
    arguments: list[str],
    candidate: RunCandidateRef,
    datasource_root: Path,
) -> RunWorkflowRuntimeResult:
    candidate_id = candidate.identifier
    candidate_path = candidate.path

    try:
        effective = workflow.parse_arguments(arguments)
    except ValueError as error:
        raise WorkflowExecutionFailure() from error

    operation = WorkflowOperation(datasource_root)
    context = WorkflowContext(operation)
    try:
        with workflow_logging(candidate_id, pack_name, workflow_name):
            try:
                value = workflow.function(context, **effective)
            except (Exception, SystemExit) as error:
                raise WorkflowExecutionFailure() from error
            try:
                outputs = materialize_outputs(value, candidate_path)
            except (Exception, SystemExit) as error:
                raise WorkflowExecutionFailure() from error
    finally:
        operation.expire()
    return RunWorkflowRuntimeResult(
        effective_inputs={
            name: _project_effective_input(value) for name, value in effective.items()
        },
        outputs=outputs,
    )


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
