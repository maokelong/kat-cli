from __future__ import annotations

from contextlib import ExitStack, contextmanager
from dataclasses import dataclass
import logging
import math
from pathlib import Path
import sys
from typing import Iterator

import kat
import pyarrow as pa
from datafusion import DataFrame, Expr, SQLOptions, SessionContext

from kat._temporal import _duration_nanoseconds, _wall_clock_nanoseconds

from .clock import ClockCapability
from .outputs import materialize_outputs
from .inspection import CompiledWorkflow
from .pack import ProductionPack
from .request import ResolvedDatasetRef, RunCandidateRef, RunWorkflowRequest
from .sources import SourceArgumentOverride, open_source_operation


@dataclass(frozen=True)
class RunWorkflowRuntimeResult:
    effective_inputs: dict[str, object]
    outputs: dict[str, dict[str, object]]


class WorkflowExecutionFailure(Exception):
    """已知且可由 PACK 作者纠正的 Workflow 解析或执行路径失败。

    包括已加载 Workflow 的参数、Dataset、用户函数与 Output
    materialization；kat_run 的 Workflow 名称查找未命中也使用此类。非法 Python
    fixture 实参及 pytest plugin、fixture、日志设施等 harness 异常不属于此类。
    """


class ExecutionLease:
    def __init__(self) -> None:
        self._active = True

    def require_active(self) -> None:
        if not self._active:
            raise RuntimeError("Workflow execution lease is no longer active")

    def expire(self) -> None:
        self._active = False


class WorkflowContext(kat.Context):
    def __init__(
        self,
        session: SessionContext,
        lease: ExecutionLease,
        clock: ClockCapability,
    ) -> None:
        self._session = session
        self._lease = lease
        self._clock = clock
        self._sql_options = (
            SQLOptions()
            .with_allow_ddl(False)
            .with_allow_dml(False)
            .with_allow_statements(False)
        )

    def sql(self, sql: str, **params: object) -> DataFrame:
        self._lease.require_active()
        if type(sql) is not str or not sql.strip():
            raise TypeError("ctx.sql requires a non-empty SQL string")
        values = {name: _sql_parameter(name, value) for name, value in params.items()}
        return self._session.sql(sql, options=self._sql_options, param_values=values)

    def from_arrow(self, table: object) -> DataFrame:
        self._lease.require_active()
        if not isinstance(table, pa.Table):
            raise TypeError("ctx.from_arrow requires a PyArrow Table")
        return self._session.from_arrow(table)

    def convert_clock(
        self,
        clock_domain: object,
        clock_value: object,
        *,
        source: str,
        target_domain: str,
        pack: str | None = None,
    ) -> Expr:
        self._lease.require_active()
        return self._clock.convert(
            clock_domain,
            clock_value,
            source=source,
            target_domain=target_domain,
            pack=pack,
        )


def run_workflow(request: RunWorkflowRequest) -> RunWorkflowRuntimeResult:
    pack_name = request.pack_name
    workflow_name = request.workflow_name
    pack = ProductionPack.open(pack_name, request.pack_path)
    workflow = pack.load(workflow_name)
    return run_loaded_workflow(
        workflow,
        pack=pack,
        workflow_name=workflow_name,
        dataset=request.dataset,
        arguments=request.arguments,
        candidate=request.candidate,
        pack_paths=request.pack_paths,
    )


def run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    pack: ProductionPack,
    workflow_name: str,
    dataset: ResolvedDatasetRef | None,
    arguments: list[str],
    candidate: RunCandidateRef,
    source_overrides: dict[str, SourceArgumentOverride] | None = None,
    pack_paths: dict[str, Path] | None = None,
) -> RunWorkflowRuntimeResult:
    candidate_id = candidate.identifier
    candidate_path = candidate.path
    pack_name = pack.name

    try:
        effective = workflow.parse_arguments(arguments)
    except ValueError as error:
        raise WorkflowExecutionFailure() from error
    with ExitStack() as source_stack:
        try:
            operation = source_stack.enter_context(
                open_source_operation(
                    current_pack=pack,
                    dataset=dataset,
                    overrides=source_overrides,
                    pack_paths=pack_paths,
                )
            )
        except (Exception, SystemExit) as error:
            raise WorkflowExecutionFailure() from error
        clock = ClockCapability(operation.session, pack.name)
        lease = ExecutionLease()
        context = WorkflowContext(operation.session, lease, clock)
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
            lease.expire()
    return RunWorkflowRuntimeResult(
        effective_inputs={
            name: _project_effective_input(value) for name, value in effective.items()
        },
        outputs=outputs,
    )


def _sql_parameter(name: str, value: object) -> object:
    if type(value) is bool or type(value) is str:
        return value
    if type(value) is int and -(2**63) <= value < 2**63:
        return value
    if type(value) is float and math.isfinite(value):
        return value
    if isinstance(value, kat.Duration):
        return _duration_nanoseconds(str(value))
    if isinstance(value, kat.WallClockTimestamp):
        return pa.scalar(
            _wall_clock_nanoseconds(str(value)), type=pa.timestamp("ns", tz="UTC")
        )
    raise TypeError(
        f"SQL parameter {name!r} must be bool, int64, finite float, str, Duration, or WallClockTimestamp"
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
