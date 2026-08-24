from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, TypeVar


F = TypeVar("F", bound=Callable[..., Any])
_REGISTRATIONS: list[Callable[..., Any]] = []


@dataclass(frozen=True)
class _SourceDeclaration:
    name: str


def source(*, name: str) -> Callable[[F], F]:
    """Declare one module-top-level synchronous KAT Source Entry.

    ``name`` identifies the DataFusion schema exposed by the Source and must
    match ``[a-z][a-z0-9]*(?:_[a-z0-9]+)*``. It must not be a Windows device
    name such as ``con``, ``com1``, or ``lpt1``. ``dataset`` is reserved by
    KAT, and ``information_schema`` is reserved by DataFusion. The decorated
    function declares only Source identity; tables belong to the returned
    provider.

    Source parameters are compiled from the function signature. They support
    the Workflow scalar input types plus ``pathlib.Path``, optional ``Path``,
    and repeated ``tuple[Path, ...]``. A Source Entry receives named values
    only; it does not receive ``kat.Context`` or an inputs mapping.

    Inspection never calls the function or evaluates its return annotation.
    When a Source operation calls it, the function must return a DataFusion
    schema provider value that KAT can register.
    """
    if type(name) is not str:
        raise TypeError("Source name must be a string")
    declaration = _SourceDeclaration(name=name)

    def decorate(function: F) -> F:
        if hasattr(function, "__kat_source__"):
            raise ValueError("a function can declare only one Source")
        setattr(function, "__kat_source__", declaration)
        _REGISTRATIONS.append(function)
        return function

    return decorate


def _registration_count() -> int:
    return len(_REGISTRATIONS)


def _registrations_since(index: int) -> tuple[Callable[..., Any], ...]:
    return tuple(_REGISTRATIONS[index:])
