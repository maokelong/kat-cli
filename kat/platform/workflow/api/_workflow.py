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
