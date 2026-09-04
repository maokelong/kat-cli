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
        """Return the shared Datasource materialization root for the current
        Analysis Session.

        Workflows in the same Session may reuse complete materializations by an
        explicitly agreed source name, including across PACKs. A Provider must
        validate a reused materialization against its own data contract and must
        not replace an already published materialization in place.

        This ordinary Path capability is valid only for this Workflow execution.
        It narrows the normal authoring interface but is not a filesystem sandbox.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")

    @property
    def scratch_root(self) -> Path:
        """Return the temporary root for the current candidate execution.

        Use it only for discardable intermediate files. KAT ensures it is
        cleaned when execution ends, and its contents must not be reused by
        later Workflows.

        This ordinary Path capability is valid only for this Workflow execution.
        It narrows the normal authoring interface but is not a filesystem sandbox.
        """
        raise RuntimeError("Context is not bound to a Workflow execution")


@dataclass(frozen=True)
class _WorkflowDeclaration:
    name: str
    description: str
    parameters: tuple[tuple[str, str], ...] | None
    guide: str | None


def workflow(
    *,
    name: str,
    description: str,
    parameters: dict[str, str] | None = None,
    guide: str | None = None,
) -> Callable[[F], F]:
    """Declare a module-top-level synchronous KAT Workflow.

    ``name`` must match ``[a-z0-9]+(?:-[a-z0-9]+)*``. ``description`` and
    every parameter description must remain non-empty after trimming outer
    whitespace. ``guide`` optionally names a Markdown file relative to the
    PACK's ``knowledge`` directory; PACK inspection validates that reference.

    The decorated function must start with ``ctx: kat.Context`` and give every
    remaining parameter exactly one description through ``parameters``.
    Supported parameter annotations are ``str``, ``int``, ``float``, ``bool``,
    ``kat.Duration``, ``kat.WallClockTimestamp``, string ``Literal`` values,
    and resolved optional non-boolean values equivalent to ``T | None``.
    Non-boolean parameters without defaults are required. Boolean parameters
    require a default, while optional parameters must default to None.
    Inspection validates and converts defaults using their CLI types.

    Duration inputs use a non-negative decimal followed by one of ``ns``,
    ``us``, ``ms``, ``s``, ``min``, or ``h``. Wall-clock inputs use RFC 3339
    with ``Z`` or a known explicit UTC offset, at most nine fractional digits,
    and never the unknown offset ``-00:00``. A ``WallClockTimestamp`` is an
    absolute UTC instant, not a local civil-time value; its input offset is
    consumed during normalization to ``Z``.

    Applying the decorator validates its argument shapes, description, guide,
    and parameter descriptions. PACK inspection then validates the Workflow
    name, callable, complete signature, annotations, description mapping, and
    converted defaults. Successful decoration alone does not mean the
    production input Interface is valid. Inspection does not evaluate or
    publish the return annotation.

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
    if not name.strip():
        raise ValueError("Workflow name must not be empty")
    if type(description) is not str:
        raise TypeError("Workflow description must be a string")
    if not description.strip():
        raise ValueError("Workflow description must not be empty")
    if guide is not None and type(guide) is not str:
        raise TypeError("Workflow guide must be a string or None")
    if guide is not None and not guide.strip():
        raise ValueError("Workflow guide must not be empty")
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
        description=description.strip(),
        parameters=normalized_parameters,
        guide=None if guide is None else guide.strip(),
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
