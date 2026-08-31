from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeVar


F = TypeVar("F", bound=Callable[..., Any])
_REGISTRATIONS: list[Callable[..., Any]] = []


class Context:
    """The KAT-owned capability boundary supplied for one Workflow execution."""

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
    parameter description must remain non-empty after trimming outer whitespace.

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
    descriptions. PACK inspection then validates the Workflow name, callable,
    docstring, complete signature, annotations,
    description mapping, and converted defaults. Successful decoration alone
    does not mean the production input Interface is valid. Inspection does
    not evaluate or publish the return annotation.

    At execution, the function must return one exact ``kat.dataprovider.Table``,
    or an exact, non-empty ``dict`` mapping Output names to exact Tables. Every
    single value becomes the ``main`` Output; a Table does not carry an Output
    name.
    Output names must match ``[a-z][a-z0-9]*(?:_[a-z0-9]+)*`` and must not
    be the Windows device names ``con``, ``prn``, ``aux``, ``nul``,
    ``com1`` through ``com9``, or ``lpt1`` through ``lpt9``. KAT validates
    the complete returned shape before materializing every Output as one
    all-or-fail Run publication.
    """
    if type(name) is not str:
        raise TypeError("Workflow name must be a string")
    if type(title) is not str or not title.strip():
        raise ValueError("Workflow title must be a non-empty string")
    if parameters is not None and (
        type(parameters) is not dict
        or any(
            type(key) is not str or type(value) is not str
            for key, value in parameters.items()
        )
    ):
        raise TypeError("parameters must be a dict of string descriptions")
    normalized_parameters: tuple[tuple[str, str], ...] | None = None
    if parameters is not None:
        normalized_parameters = tuple(
            (key, value.strip()) for key, value in parameters.items()
        )
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
