from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
import re
from typing import Any, TypeVar


F = TypeVar("F", bound=Callable[..., Any])
_REGISTRATIONS: list[Callable[..., Any]] = []
_TABLE_NAME = re.compile(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*\Z")
_WINDOWS_DEVICES = {"con", "prn", "aux", "nul"} | {
    f"{prefix}{number}" for prefix in ("com", "lpt") for number in range(1, 10)
}


class Context:
    """The KAT-owned capability boundary supplied when a Workflow is executed."""


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
    not evaluate or publish the return annotation; output validation belongs
    to the later Workflow execution boundary.
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
        if _TABLE_NAME.fullmatch(table) is None or table in _WINDOWS_DEVICES:
            raise ValueError(f"invalid Required table name: {table!r}")
    return normalized
