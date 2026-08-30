from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeVar

from datafusion import DataFrame, Expr
from pyarrow import Table

from ._identifiers import valid_table_name
from ._temporal import Duration, WallClockTimestamp


F = TypeVar("F", bound=Callable[..., Any])
_REGISTRATIONS: list[Callable[..., Any]] = []
class Context:
    """The KAT-owned capability boundary supplied for one Workflow execution.

    ``required_tables`` controls the Dataset tables visible to Workflow SQL.
    KAT-owned capabilities may read private platform evidence without registering
    those evidence tables in the Workflow session.
    """

    def sql(
        self,
        sql: str,
        **params: bool | int | float | str | Duration | WallClockTimestamp,
    ) -> DataFrame:
        """Build a DataFusion DataFrame from one read-only SQL statement.

        DataFusion performs parsing and planning. KAT disables DDL, DML, COPY,
        session mutation, and multiple statements while retaining DataFusion's
        read-only ``SHOW``, ``DESCRIBE``, and ``EXPLAIN`` statements. ``$name``
        value placeholders bind only exact keyword parameters of type ``bool``,
        signed int64, finite ``float``, ``str``, ``Duration``, or
        ``WallClockTimestamp``; they never substitute identifiers or SQL text.

        Context methods may be called only during the current Workflow
        execution. DataFrames are lazy and must be returned by the Workflow so
        KAT can materialize them before that execution closes.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")

    def from_arrow(self, table: Table) -> DataFrame:
        """Expose one PyArrow Table as a DataFusion DataFrame.

        Other Arrow containers and table-like Python objects are not accepted.
        Context methods may be called only during the current Workflow
        execution. DataFrames are lazy and must be returned by the Workflow so
        KAT can materialize them before that execution closes.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")

    @property
    def datasource_root(self) -> Path:
        """Return this PACK's private Datasource storage root.

        Production executions receive
        ``KAT_DATA_HOME/datasources/<pack-name>/``. PACK tests receive a root
        isolated to the current pytest test. The path capability is valid only
        for this Workflow execution. File Providers should create a temporary
        per-Workflow workspace below it instead of treating old files as cache.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")

    def convert_clock(
        self,
        clock_domain: Expr,
        clock_value: Expr,
        *,
        target_domain: str,
    ) -> Expr:
        """Translate a ClockValue to one fixed target domain.

        ``target_domain`` must have exact type ``str`` and be non-empty; ``None``,
        other types, and ``str`` subclasses are rejected before the Expr is
        constructed. ``clock_domain`` and ``clock_value`` must be Expr values
        accepted by DataFusion's strict casts to Arrow ``Utf8`` and ``UInt64``.
        KAT guarantees canonical ``Utf8``/``UInt64``, ``LargeUtf8``/``Utf8View``
        domains, and representable non-negative ``Int64`` values; negative,
        overflowing, or invalid text values fail the Workflow. Other source
        types are not part of the Pack Authoring Interface even if the pinned
        engine can cast them. Clock conversion is not registered as a SQL
        function.

        Every first-version Clock domain definition in the Dataset must use an
        admitted clock type at exactly 1 GHz. Conversion preserves values already
        in the exact target domain after validating those definitions.
        Cross-domain conversion additionally uses only Dataset
        ``snapshot_id = 0`` as a constant offset. It does not scale different
        frequencies, follow multiple hops, or correct drift.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")


@dataclass(frozen=True)
class _WorkflowDeclaration:
    name: str
    title: str
    required_tables: tuple[str, ...]
    parameters: tuple[tuple[str, str], ...] | None


def workflow(
    *,
    name: str,
    title: str,
    required_tables: list[str],
    parameters: dict[str, str] | None = None,
) -> Callable[[F], F]:
    """Declare a module-top-level synchronous KAT Workflow.

    ``name`` must match ``[a-z0-9]+(?:-[a-z0-9]+)*``. Each
    ``required_tables`` item must match
    ``[a-z][a-z0-9]*(?:_[a-z0-9]+)*`` and must not be a reserved Windows
    device name. ``title`` and every parameter description must remain
    non-empty after trimming outer whitespace.

    The decorated function must have a non-empty docstring, start with
    ``ctx: kat.Context``, and give every remaining parameter exactly one
    description through ``parameters``. Supported parameter annotations are
    ``str``, ``int``, ``float``, ``bool``, ``kat.Duration``,
    ``kat.WallClockTimestamp``, string ``Literal`` values, and resolved
    optional non-boolean values equivalent to ``T | None``. Non-boolean
    parameters without defaults are required. Boolean parameters require a
    default, while optional parameters must default to None. Inspection
    validates and converts defaults using their CLI types.

    Duration inputs use a non-negative decimal followed by one of ``ns``,
    ``us``, ``ms``, ``s``, ``min``, or ``h``. Wall-clock inputs use RFC 3339
    with ``Z`` or a known explicit UTC offset, at most nine fractional digits,
    and never the unknown offset ``-00:00``. A ``WallClockTimestamp`` is an
    absolute UTC instant, not a local civil-time value; its input offset is
    consumed during normalization to ``Z``.

    Applying the decorator validates its argument shapes, title, parameter
    descriptions, and Required table names. PACK inspection then validates
    the Workflow name, callable, docstring, complete signature, annotations,
    description mapping, and converted defaults. Successful decoration alone
    does not mean the production input Interface is valid. Inspection does
    not evaluate or publish the return annotation.

    At execution, the function must return one ``kat.datasource.Table``, or an
    exact, non-empty ``dict`` mapping Output names to Tables. During migration,
    one DataFusion ``DataFrame`` or a dict mixing Tables and DataFrames is also
    accepted. Every single value becomes the ``main`` Output; a Table does not
    carry an Output name.
    Output names must match ``[a-z][a-z0-9]*(?:_[a-z0-9]+)*`` and must not
    be the Windows device names ``con``, ``prn``, ``aux``, ``nul``,
    ``com1`` through ``com9``, or ``lpt1`` through ``lpt9``. KAT validates
    the complete returned shape before executing any lazy Output plan, then
    materializes every Output as one all-or-fail Run publication.
    """
    if type(name) is not str:
        raise TypeError("Workflow name must be a string")
    if type(title) is not str or not title.strip():
        raise ValueError("Workflow title must be a non-empty string")
    if type(required_tables) is not list or any(type(item) is not str for item in required_tables):
        raise TypeError("required_tables must be a list of strings")
    if parameters is not None and (
        type(parameters) is not dict
        or any(type(key) is not str or type(value) is not str for key, value in parameters.items())
    ):
        raise TypeError("parameters must be a dict of string descriptions")
    normalized_parameters: tuple[tuple[str, str], ...] | None = None
    if parameters is not None:
        normalized_parameters = tuple((key, value.strip()) for key, value in parameters.items())
        if any(not value for _, value in normalized_parameters):
            raise ValueError("Workflow parameter descriptions must not be empty")
    declaration = _WorkflowDeclaration(
        name=name,
        title=title.strip(),
        required_tables=_normalize_required_tables(required_tables),
        parameters=normalized_parameters,
    )

    def decorate(function: F) -> F:
        if hasattr(function, "__kat_workflow__"):
            raise ValueError("a function can declare only one Workflow")
        setattr(function, "__kat_workflow__", declaration)
        _REGISTRATIONS.append(function)
        return function

    return decorate


def _registration_count() -> int:
    return len(_REGISTRATIONS)


def _registrations_since(index: int) -> tuple[Callable[..., Any], ...]:
    return tuple(_REGISTRATIONS[index:])


def _normalize_required_tables(required_tables: list[str] | tuple[str, ...]) -> tuple[str, ...]:
    normalized = tuple(sorted(set(required_tables)))
    for table in normalized:
        if not valid_table_name(table):
            raise ValueError(f"invalid Required table name: {table!r}")
    return normalized
