from __future__ import annotations

from collections.abc import Mapping
from contextlib import contextmanager
from dataclasses import dataclass
import logging
from pathlib import Path
import sys
from typing import Iterator

import kat
import pyarrow as pa
from datafusion import DataFrame, Expr, SessionContext

from kat.datasource import Table, to_arrow
from kat.datasource._sql import execute_sql, require_sql_name
from kat._temporal import _duration_nanoseconds

from .clock import ClockCapability
from .datasource import WorkflowOperation
from .outputs import materialize_outputs
from .inspection import CompiledWorkflow
from .pack import ProductionPack
from .request import ResolvedDatasetRef, RunCandidateRef, RunWorkflowRequest


@dataclass(frozen=True)
class RunWorkflowRuntimeResult:
    effective_inputs: dict[str, object]
    outputs: dict[str, dict[str, object]]


class WorkflowExecutionFailure(Exception):
    """已知且可由 PACK 作者纠正的 Workflow 解析或执行路径失败。

    包括已加载 Workflow 的参数、Dataset/Table Grant、用户函数与 Output
    materialization；kat_run 的 Workflow 名称查找未命中也使用此类。非法 Python
    fixture 实参及 pytest plugin、fixture、日志设施等 harness 异常不属于此类。
    """


class WorkflowContext(kat.Context):
    def __init__(
        self,
        session: SessionContext,
        operation: WorkflowOperation,
        clock: ClockCapability,
    ) -> None:
        self._session = session
        self._operation = operation
        self._clock = clock

    def sql(
        self,
        sql: str,
        *,
        tables: Mapping[str, Table] | None = None,
        params: Mapping[str, object] | None = None,
    ) -> Table:
        self._operation.require_active()
        explicit_tables = _fusion_tables(tables)
        dataset_tables = self._operation.dataset_tables
        conflicts = sorted(set(explicit_tables) & set(dataset_tables))
        if conflicts:
            raise ValueError(
                "Fusion relation names conflict with Dataset grants: "
                + ", ".join(conflicts)
            )

        session = SessionContext()
        for name, path in dataset_tables.items():
            session.register_parquet(name, str(path))
        for name, table in explicit_tables.items():
            session.from_arrow(to_arrow(table), name=name)
        return execute_sql(session, sql, params=params)

    def from_arrow(self, table: object) -> DataFrame:
        self._operation.require_active()
        if not isinstance(table, pa.Table):
            raise TypeError("ctx.from_arrow requires a PyArrow Table")
        return self._session.from_arrow(table)

    def convert_clock(
        self,
        clock_domain: object,
        clock_value: object,
        *,
        target_domain: str,
    ) -> Expr:
        self._operation.require_active()
        return self._clock.convert(
            clock_domain,
            clock_value,
            target_domain=target_domain,
        )

    @property
    def datasource_root(self) -> Path:
        return self._operation.datasource_root


def run_workflow(request: RunWorkflowRequest) -> RunWorkflowRuntimeResult:
    pack_name = request.pack_name
    workflow_name = request.workflow_name
    candidate_id = request.candidate.identifier
    candidate_path = request.candidate.path
    dataset = request.dataset

    workflow = ProductionPack.open(pack_name, request.pack_path).load(workflow_name)
    return run_loaded_workflow(
        workflow,
        pack_name=pack_name,
        workflow_name=workflow_name,
        dataset=dataset,
        arguments=request.arguments,
        candidate=request.candidate,
        datasource_root=request.datasource_root,
    )


def run_loaded_workflow(
    workflow: CompiledWorkflow,
    *,
    pack_name: str,
    workflow_name: str,
    dataset: ResolvedDatasetRef | None,
    arguments: list[str],
    candidate: RunCandidateRef,
    datasource_root: Path,
) -> RunWorkflowRuntimeResult:
    candidate_id = candidate.identifier
    candidate_path = candidate.path

    required_tables = workflow.interface["required_tables"]
    table_paths = {} if dataset is None else dataset.tables
    if required_tables and dataset is None:
        raise WorkflowExecutionFailure() from ValueError(
            "the selected Workflow requires a Dataset"
        )
    missing = sorted(set(required_tables) - set(table_paths))
    if missing:
        raise WorkflowExecutionFailure() from ValueError(
            f"Dataset is missing required tables: {', '.join(missing)}"
        )

    try:
        effective = workflow.parse_arguments(arguments)
    except ValueError as error:
        raise WorkflowExecutionFailure() from error
    session = SessionContext()
    try:
        for table_name in required_tables:
            session.register_parquet(table_name, str(table_paths[table_name]))
    except (Exception, SystemExit) as error:
        raise WorkflowExecutionFailure() from error
    clock = ClockCapability(dataset)

    operation = WorkflowOperation(
        candidate_path,
        datasource_root,
        {name: table_paths[name] for name in required_tables},
    )
    context = WorkflowContext(session, operation, clock)
    try:
        with workflow_logging(candidate_id, pack_name, workflow_name):
            try:
                value = workflow.function(context, **effective)
            except (Exception, SystemExit) as error:
                raise WorkflowExecutionFailure() from error
            try:
                outputs = materialize_outputs(value, operation)
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


def _fusion_tables(tables: Mapping[str, Table] | None) -> dict[str, Table]:
    if tables is None:
        return {}
    if not isinstance(tables, Mapping):
        raise TypeError("ctx.sql tables must be a mapping or None")
    snapshot = dict(tables.items())
    result: dict[str, Table] = {}
    for name, table in snapshot.items():
        relation_name = require_sql_name(name, "Fusion relation")
        if not isinstance(table, Table):
            raise TypeError(
                f"Fusion relation {relation_name!r} must be a datasource.Table"
            )
        result[relation_name] = table
    return result


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
