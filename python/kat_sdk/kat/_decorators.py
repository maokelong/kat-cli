from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, TypeVar

F = TypeVar("F", bound=Callable[..., Any])


@dataclass(frozen=True)
class OptionSpec:
    flags: tuple[str, ...]
    help: str = ""
    default: Any = None
    required: bool = False

    @property
    def name(self) -> str:
        if not self.flags:
            return ""
        flag = self.flags[0]
        return flag.lstrip("-").replace("-", "_")


@dataclass(frozen=True)
class WorkflowSpec:
    title: str = ""
    name: str | None = None
    description: str = ""
    options: tuple[OptionSpec, ...] = field(default_factory=tuple)


def option(
    *flags: str,
    help: str = "",
    default: Any = None,
    required: bool = False,
) -> Callable[[F], F]:
    if not flags:
        raise ValueError("option requires at least one flag")
    spec = OptionSpec(flags=tuple(flags), help=help, default=default, required=required)

    def decorate(fn: F) -> F:
        existing = list(getattr(fn, "__kat_options__", ()))
        existing.insert(0, spec)
        setattr(fn, "__kat_options__", tuple(existing))
        _refresh_workflow_options(fn)
        return fn

    return decorate


def workflow(
    *,
    title: str = "",
    name: str | None = None,
    description: str = "",
) -> Callable[[F], F]:
    def decorate(fn: F) -> F:
        options = tuple(getattr(fn, "__kat_options__", ()))
        setattr(
            fn,
            "__kat_workflow__",
            WorkflowSpec(title=title, name=name, description=description, options=options),
        )
        return fn

    return decorate


def get_workflow_spec(fn: Callable[..., Any]) -> WorkflowSpec | None:
    spec = getattr(fn, "__kat_workflow__", None)
    if spec is None:
        return None
    if not isinstance(spec, WorkflowSpec):
        raise TypeError("__kat_workflow__ is not a WorkflowSpec")
    return spec


def _refresh_workflow_options(fn: Callable[..., Any]) -> None:
    spec = getattr(fn, "__kat_workflow__", None)
    if spec is None:
        return
    setattr(
        fn,
        "__kat_workflow__",
        WorkflowSpec(
            title=spec.title,
            name=spec.name,
            description=spec.description,
            options=tuple(getattr(fn, "__kat_options__", ())),
        ),
    )
