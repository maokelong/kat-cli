from __future__ import annotations

from collections.abc import Sequence
from contextlib import contextmanager
import logging
import math
from pathlib import Path
import re
import sys
from typing import Iterator
import uuid

import kat
import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq
from datafusion import DataFrame, Expr, SQLOptions, SessionContext, lit, udf

from kat._temporal import _duration_nanoseconds, _wall_clock_nanoseconds

from .outputs import publish_outputs
from .pack import load_workflows


_DOMAIN = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")
_CLOCK_TYPES = {
    "boottime",
    "monotonic",
    "monotonic_coarse",
    "monotonic_raw",
    "realtime",
    "realtime_coarse",
    "ftrace_global",
    "ftrace_local",
}


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
        clock_udf: object,
    ) -> None:
        self._session = session
        self._lease = lease
        self._clock_udf = clock_udf
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
        unbound = self._session.sql(sql, options=self._sql_options)
        _validate_clock_target_literals(unbound)
        values = {name: _sql_parameter(name, value) for name, value in params.items()}
        frame = self._session.sql(sql, options=self._sql_options, param_values=values)
        _validate_clock_target_literals(frame)
        return frame

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
        if not isinstance(clock_domain, Expr) or not isinstance(clock_value, Expr):
            raise TypeError("ctx.convert_clock requires DataFusion Expr inputs")
        if type(target_domain) is not str or not target_domain:
            raise TypeError("ctx.convert_clock target_domain must be a non-empty string")
        return self._clock_udf(clock_domain, clock_value, lit(target_domain))


def run_workflow(request: dict[str, object]) -> dict[str, object]:
    pack_name = _string(request, "pack_name")
    pack_path = _string(request, "pack_path")
    workflow_name = _string(request, "workflow_name")
    arguments = request["arguments"]
    if type(arguments) is not list or any(type(value) is not str for value in arguments):
        raise TypeError("run_workflow arguments must be an array of strings")
    candidate_id = _string(request, "candidate_id")
    run_path = _candidate_run_path(candidate_id, _string(request, "run_path"))
    dataset = _resolved_dataset(request.get("dataset"))

    workflows = load_workflows(pack_name, pack_path)
    return execute_loaded_workflow(
        workflows,
        pack_name=pack_name,
        workflow_name=workflow_name,
        arguments=arguments,
        candidate_id=candidate_id,
        run_path=run_path,
        dataset=dataset,
    )


def execute_loaded_workflow(
    workflows: dict[str, object],
    *,
    pack_name: str,
    workflow_name: str,
    arguments: list[str],
    candidate_id: str,
    run_path: Path,
    dataset: dict[str, object] | None,
) -> dict[str, object]:
    try:
        workflow = workflows[workflow_name]
    except KeyError as error:
        raise ValueError(f"Workflow {workflow_name!r} was not found in the selected PACK") from error

    required_tables = workflow.interface["required_tables"]
    table_paths = {} if dataset is None else dataset["tables"]
    if required_tables and dataset is None:
        raise ValueError("the selected Workflow requires a Dataset")
    missing = sorted(set(required_tables) - set(table_paths))
    if missing:
        raise ValueError(f"Dataset is missing required tables: {', '.join(missing)}")

    effective = workflow.parse_arguments(arguments)
    session = SessionContext()
    for table_name in required_tables:
        session.register_parquet(table_name, table_paths[table_name])
    resolver = ClockResolver(dataset)
    clock_udf = udf(
        resolver.convert,
        [pa.string(), pa.uint64(), pa.string()],
        pa.uint64(),
        "stable",
        name="kat_convert_clock",
    )
    session.register_udf(clock_udf)

    lease = ExecutionLease()
    context = WorkflowContext(session, lease, clock_udf)
    try:
        with workflow_logging(candidate_id, pack_name, workflow_name):
            value = workflow.function(context, **effective)
            outputs = publish_outputs(value, run_path)
    finally:
        lease.expire()
    return {
        "effective_inputs": {
            name: _project_effective_input(value) for name, value in effective.items()
        },
        "outputs": outputs,
    }


class TestWorkflowFailure(Exception):
    pass


def execute_test_workflow(
    workflows: dict[str, object],
    *,
    pack_name: str,
    workflow_name: object,
    arguments: object,
    candidate_id: str,
    run_path: Path,
    dataset: object,
) -> dict[str, pa.Table]:
    if type(workflow_name) is not str or not workflow_name:
        raise TestWorkflowFailure("KAT Workflow test execution failed") from TypeError(
            "kat_run workflow must be a non-empty string"
        )
    if isinstance(arguments, (str, bytes)) or not isinstance(arguments, Sequence):
        raise TestWorkflowFailure("KAT Workflow test execution failed") from TypeError(
            "kat_run arguments must be a sequence of strings"
        )
    if any(type(argument) is not str for argument in arguments):
        raise TestWorkflowFailure("KAT Workflow test execution failed") from TypeError(
            "kat_run arguments must be a sequence of strings"
        )
    resolved_path = _candidate_run_path(candidate_id, str(run_path))
    resolved_dataset = _resolved_dataset(dataset)
    try:
        result = execute_loaded_workflow(
            workflows,
            pack_name=pack_name,
            workflow_name=workflow_name,
            arguments=list(arguments),
            candidate_id=candidate_id,
            run_path=resolved_path,
            dataset=resolved_dataset,
        )
    except Exception as error:
        raise TestWorkflowFailure("KAT Workflow test execution failed") from error
    return {
        name: pq.read_table(
            resolved_path / "outputs" / f"{output['output_id']}.parquet"
        )
        for name, output in result["outputs"].items()
    }


def _validate_clock_target_literals(frame: DataFrame) -> None:
    pending = [frame.logical_plan()]
    while pending:
        plan = pending.pop()
        pending.extend(plan.inputs())
        for expression in _plan_expressions(plan.to_variant()):
            _validate_clock_expression(expression)


def _validate_clock_expression(expression: object) -> None:
    if "kat_convert_clock" not in repr(expression):
        return
    kind = expression.variant_name()
    if kind in {"AggregateFunction", "ScalarFunction", "WindowFunction"}:
        operator = expression.rex_call_operator()
        operands = expression.rex_call_operands()
    else:
        operator = None
        operands = []
    if operator == "kat_convert_clock":
        if len(operands) != 3:
            raise ValueError("kat_convert_clock requires exactly three arguments")
        target = operands[2]
        if target.variant_name() != "Literal":
            raise ValueError("kat_convert_clock target_domain must be a string literal")
        try:
            value = target.python_value()
        except ValueError as error:
            raise ValueError(
                "kat_convert_clock target_domain must be a string literal"
            ) from error
        if isinstance(value, pa.Scalar):
            value = value.as_py()
        if type(value) is not str or not value:
            raise ValueError("kat_convert_clock target_domain must be a string literal")
    for operand in operands:
        _validate_clock_expression(operand)
    if operands:
        return
    if kind in {
        "Column",
        "Literal",
        "OuterReferenceColumn",
        "Placeholder",
        "ScalarVariable",
        "Wildcard",
    }:
        return
    try:
        variant = expression.to_variant()
    except ValueError as error:
        if "kat_convert_clock" in repr(expression):
            raise ValueError(
                "kat_convert_clock is nested in an unsupported SQL expression"
            ) from error
        return
    children = {
        "Alias": lambda: [variant.expr()],
        "Between": lambda: [variant.expr(), variant.low(), variant.high()],
        "BinaryExpr": lambda: [variant.left(), variant.right()],
        "Cast": lambda: [variant.expr()],
        "InList": lambda: [variant.expr(), variant.list()],
        "IsFalse": lambda: [variant.expr()],
        "IsNotFalse": lambda: [variant.expr()],
        "IsNotNull": lambda: [variant.expr()],
        "IsNotTrue": lambda: [variant.expr()],
        "IsNotUnknown": lambda: [variant.expr()],
        "IsNull": lambda: [variant.expr()],
        "IsTrue": lambda: [variant.expr()],
        "IsUnknown": lambda: [variant.expr()],
        "Like": lambda: [variant.expr(), variant.pattern()],
        "Negative": lambda: [variant.expr()],
        "Not": lambda: [variant.expr()],
        "SimilarTo": lambda: [variant.expr(), variant.pattern()],
        "TryCast": lambda: [variant.expr()],
    }.get(kind)
    if children is None:
        if kind == "Case":
            values = [variant.expr(), variant.when_then_expr(), variant.else_expr()]
        elif "kat_convert_clock" in repr(expression):
            raise ValueError(
                "kat_convert_clock is nested in an unsupported SQL expression"
            )
        else:
            return
    else:
        values = children()
    for child in _raw_expressions(values):
        _validate_clock_expression(child)


def _plan_expressions(variant: object) -> Iterator[object]:
    kind = variant.__class__.__name__
    if kind == "Projection":
        values = variant.projections()
    elif kind == "Filter":
        values = [variant.predicate()]
    elif kind == "Aggregate":
        values = [variant.group_by_exprs(), variant.aggregate_exprs()]
    elif kind == "Join":
        values = [variant.on(), variant.filter()]
    elif kind == "Sort":
        values = variant.sort_exprs()
    elif kind == "WindowExpr":
        windows = variant.get_window_expr()
        values = []
        for window in windows:
            values.extend(
                [
                    variant.get_args(window),
                    variant.get_partition_exprs(window),
                    variant.get_sort_exprs(window),
                ]
            )
    elif kind == "Repartition":
        values = variant.distribute_list()
    elif kind == "Values":
        values = variant.values()
    elif kind in {
        "Distinct",
        "EmptyRelation",
        "Limit",
        "SubqueryAlias",
        "TableScan",
        "Union",
    }:
        return
    elif "kat_convert_clock" in repr(variant):
        raise ValueError("kat_convert_clock is nested in an unsupported SQL plan")
    else:
        return
    yield from _raw_expressions(values)


def _raw_expressions(value: object) -> Iterator[object]:
    if value.__class__.__name__ == "RawExpr" and hasattr(value, "variant_name"):
        yield value
        return
    if value.__class__.__name__ == "SortExpr":
        yield value.expr()
        return
    if isinstance(value, (list, tuple)):
        for item in value:
            yield from _raw_expressions(item)


def _string(request: dict[str, object], name: str) -> str:
    value = request[name]
    if type(value) is not str or not value:
        raise TypeError(f"run_workflow {name} must be a non-empty string")
    return value


def _candidate_run_path(candidate_id: str, value: str) -> Path:
    try:
        identity = uuid.UUID(candidate_id)
    except ValueError as error:
        raise ValueError("Run candidate identity must be UUIDv7") from error
    if identity.version != 7 or str(identity) != candidate_id:
        raise ValueError("Run candidate identity must be canonical UUIDv7")
    path = Path(value)
    if not path.is_absolute():
        raise ValueError("Run candidate path must be absolute")
    resolved = path.resolve(strict=True)
    if resolved != path or not resolved.is_dir() or resolved.name != candidate_id:
        raise ValueError("Run candidate identity and directory do not match")
    if (resolved / "manifest.json").exists():
        raise ValueError("Run candidate is already published")
    return resolved


def _resolved_dataset(value: object) -> dict[str, object] | None:
    if value is None:
        return None
    if type(value) is not dict or set(value) != {"path", "tables"}:
        raise ValueError("Resolved Dataset must contain exactly path and tables")
    path = value["path"]
    tables = value["tables"]
    if type(path) is not str or not Path(path).is_absolute():
        raise ValueError("Resolved Dataset path must be absolute")
    if type(tables) is not dict:
        raise TypeError("Resolved Dataset tables must be an object")
    for name, table_path in tables.items():
        if type(name) is not str or type(table_path) is not str or not Path(table_path).is_absolute():
            raise TypeError("Resolved Dataset table references must be absolute string paths")
    return {"path": path, "tables": tables}


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


class ClockResolver:
    def __init__(
        self,
        dataset: dict[str, object] | None,
        *,
        query_dataset: object | None = None,
    ) -> None:
        self._dataset = dataset
        self._query_dataset = query_dataset
        self._definitions: dict[str, tuple[str, int]] | None = None
        self._baseline: dict[str, int] | None = None

    def convert(self, domains: object, values: object, targets: object) -> pa.Array:
        domain_array = _arrow_array(domains, pa.string())
        value_array = _arrow_array(values, pa.uint64())
        target_array = _arrow_array(targets, pa.string(), length=len(domain_array))
        if len(domain_array) != len(value_array) or len(domain_array) != len(target_array):
            raise ValueError("clock conversion inputs must have equal lengths")
        target_values = pc.unique(target_array).to_pylist()
        if len(target_values) != 1 or type(target_values[0]) is not str or not target_values[0]:
            raise ValueError("clock conversion target_domain must be one non-empty literal")
        target = target_values[0]

        half_null = pc.xor(pc.is_null(domain_array), pc.is_null(value_array))
        if pc.any(half_null).equals(pa.scalar(True)):
            raise ValueError("clock_domain and clock_value must be null together")
        definitions, baseline = self._evidence()
        if target not in definitions:
            raise ValueError(
                f"unknown target clock domain {target!r}; available domains: {', '.join(sorted(definitions))}"
            )

        result = pa.nulls(len(domain_array), type=pa.uint64())
        for source in pc.unique(pc.drop_null(domain_array)).to_pylist():
            if source not in definitions:
                raise ValueError(f"unknown source clock domain {source!r}")
            mask = pc.equal(domain_array, pa.scalar(source))
            if source == target:
                converted = value_array
            else:
                if definitions[source][1] != 1_000_000_000 or definitions[target][1] != 1_000_000_000:
                    raise ValueError("clock conversion requires one-billion ticks per second")
                if source not in baseline or target not in baseline:
                    raise ValueError("clock conversion baseline is incomplete")
                converted = _translate_clock(
                    value_array, mask, baseline[source], baseline[target]
                )
            result = pc.if_else(mask, converted, result)
        return result

    def _evidence(self) -> tuple[dict[str, tuple[str, int]], dict[str, int]]:
        if self._definitions is not None and self._baseline is not None:
            return self._definitions, self._baseline
        if self._dataset is None:
            if type(self._query_dataset) is dict:
                status = self._query_dataset.get("status")
                if status == "unavailable":
                    raise ValueError(
                        "clock conversion requires the current Dataset, which is unavailable: "
                        f"{self._query_dataset['cause']}; pure output.* queries remain available"
                    )
                if status == "not_provided":
                    raise ValueError(
                        "clock conversion requires a Dataset, but this Run did not provide one; "
                        "pure output.* queries remain available"
                    )
            raise ValueError("clock conversion requires a Dataset")
        tables = self._dataset["tables"]
        if "clock_domain" not in tables:
            raise ValueError("Dataset does not contain clock_domain evidence")
        definitions_table = pq.read_table(tables["clock_domain"])
        expected = pa.schema(
            [
                pa.field("clock_domain", pa.string(), nullable=False),
                pa.field("clock_type", pa.string(), nullable=False),
                pa.field("ticks_per_second", pa.uint64(), nullable=False),
            ]
        )
        if not definitions_table.schema.equals(expected, check_metadata=False):
            raise ValueError("clock_domain table has an invalid Schema")
        definitions: dict[str, tuple[str, int]] = {}
        for row in definitions_table.to_pylist():
            name = row["clock_domain"]
            if (
                name in definitions
                or _DOMAIN.fullmatch(name) is None
                or row["clock_type"] not in _CLOCK_TYPES
                or row["ticks_per_second"] != 1_000_000_000
            ):
                raise ValueError("clock_domain definitions are invalid")
            definitions[name] = (row["clock_type"], row["ticks_per_second"])

        baseline: dict[str, int] = {}
        if "clock_snapshot" in tables:
            snapshots = pq.read_table(tables["clock_snapshot"])
            expected_snapshot = pa.schema(
                [
                    pa.field("snapshot_id", pa.uint64(), nullable=False),
                    pa.field("clock_domain", pa.string(), nullable=False),
                    pa.field("clock_value", pa.uint64(), nullable=False),
                ]
            )
            if not snapshots.schema.equals(expected_snapshot, check_metadata=False):
                raise ValueError("clock_snapshot table has an invalid Schema")
            for row in snapshots.to_pylist():
                if row["snapshot_id"] != 0:
                    continue
                name = row["clock_domain"]
                if name not in definitions or name in baseline:
                    raise ValueError("clock_snapshot baseline has invalid domains")
                baseline[name] = row["clock_value"]
        self._definitions = definitions
        self._baseline = baseline
        return definitions, baseline


def _arrow_array(value: object, data_type: pa.DataType, length: int | None = None) -> pa.Array:
    if isinstance(value, pa.Array):
        if value.type != data_type:
            raise TypeError(f"clock conversion requires {data_type}, got {value.type}")
        return value
    if isinstance(value, pa.Scalar):
        if value.type != data_type or length is None:
            raise TypeError(f"clock conversion requires {data_type}")
        return pa.repeat(value, length)
    raise TypeError("clock conversion requires Arrow arrays or scalars")


def _translate_clock(
    values: pa.Array, mask: pa.Array, source_base: int, target_base: int
) -> pa.Array:
    safe = pc.if_else(mask, values, pa.scalar(source_base, type=pa.uint64()))
    goes_up = pc.greater_equal(safe, pa.scalar(source_base, type=pa.uint64()))
    up_values = pc.if_else(goes_up, safe, pa.scalar(source_base, type=pa.uint64()))
    down_values = pc.if_else(goes_up, pa.scalar(source_base, type=pa.uint64()), safe)
    upward = pc.add_checked(
        pa.scalar(target_base, type=pa.uint64()),
        pc.subtract_checked(up_values, pa.scalar(source_base, type=pa.uint64())),
    )
    downward = pc.subtract_checked(
        pa.scalar(target_base, type=pa.uint64()),
        pc.subtract_checked(pa.scalar(source_base, type=pa.uint64()), down_values),
    )
    return pc.if_else(goes_up, upward, downward)


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
