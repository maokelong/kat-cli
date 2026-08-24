from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, TypeVar

from datafusion import DataFrame, Expr
from pyarrow import Table

from ._temporal import Duration, WallClockTimestamp


F = TypeVar("F", bound=Callable[..., Any])
_REGISTRATIONS: list[Callable[..., Any]] = []
class Context:
    """The KAT-owned capability boundary supplied for one Workflow execution.

    Bound Dataset Sources are available through DataFusion catalog and schema
    names. KAT-owned capabilities may read private platform evidence without
    registering those evidence tables in the Workflow session.
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

    def convert_clock(
        self,
        clock_domain: Expr,
        clock_value: Expr,
        *,
        source: str,
        target_domain: str,
        pack: str | None = None,
    ) -> Expr:
        """Translate a Source-owned ClockValue to one fixed target domain.

        ``source`` names the Source schema that owns ``clock_domain`` and
        ``clock_snapshot``. ``pack=None`` selects the current Workflow PACK;
        cross-PACK conversion must provide the exact PACK identity. ``source``
        and ``target_domain`` must have exact type ``str`` and be non-empty;
        ``pack`` must be ``None`` or an exact non-empty ``str``. String
        subclasses and other types are rejected. ``clock_domain`` and
        ``clock_value`` must be Expr values accepted by DataFusion's strict
        casts to Arrow ``Utf8`` and ``UInt64``.
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
    parameters: tuple[tuple[str, str], ...] | None


def workflow(
    *,
    name: str,
    title: str,
    parameters: dict[str, str] | None = None,
) -> Callable[[F], F]:
    """Declare a module-top-level synchronous KAT Workflow.

    ``name`` must match ``[a-z0-9]+(?:-[a-z0-9]+)*``. ``title`` and every
    parameter description must remain non-empty after trimming outer
    whitespace. Dataset Sources are addressed by the Workflow implementation;
    the declaration does not repeat a static table list.

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

    Applying the decorator validates its argument shapes, title, and parameter
    descriptions. PACK inspection then validates
    the Workflow name, callable, docstring, complete signature, annotations,
    description mapping, and converted defaults. Successful decoration alone
    does not mean the production input Interface is valid. Inspection does
    not evaluate or publish the return annotation.

    At execution, the function must return either one DataFusion ``DataFrame``
    or an exact, non-empty ``dict`` mapping Output names to DataFusion
    ``DataFrame`` values. A single DataFrame becomes the ``main`` Output.
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
