from __future__ import annotations

from contextlib import contextmanager
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
from .pack import ProductionPack
from .request import RunWorkflowRequest


@dataclass(frozen=True)
class RunWorkflowRuntimeResult:
    effective_inputs: dict[str, object]
    outputs: dict[str, dict[str, object]]


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
        target_domain: str,
    ) -> Expr:
        self._lease.require_active()
        return self._clock.convert(
            clock_domain,
            clock_value,
            target_domain=target_domain,
        )


def run_workflow(request: RunWorkflowRequest) -> RunWorkflowRuntimeResult:
    pack_name = request.pack_name
    workflow_name = request.workflow_name
    candidate_id = request.candidate.identifier
    candidate_path = request.candidate.path
    dataset = request.dataset

    workflow = ProductionPack.open(pack_name, request.pack_path).load(workflow_name)

    required_tables = workflow.interface["required_tables"]
    table_paths = {} if dataset is None else dataset.tables
    if required_tables and dataset is None:
        raise ValueError("the selected Workflow requires a Dataset")
    missing = sorted(set(required_tables) - set(table_paths))
    if missing:
        raise ValueError(f"Dataset is missing required tables: {', '.join(missing)}")

    effective = workflow.parse_arguments(request.arguments)
    session = SessionContext()
    for table_name in required_tables:
        session.register_parquet(table_name, str(table_paths[table_name]))
    clock = ClockCapability(dataset)

    lease = ExecutionLease()
    context = WorkflowContext(session, lease, clock)
    try:
        with workflow_logging(candidate_id, pack_name, workflow_name):
            value = workflow.function(context, **effective)
            outputs = materialize_outputs(value, candidate_path)
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
