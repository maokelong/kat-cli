from __future__ import annotations

from collections.abc import Callable
from typing import Any, TypeVar


F = TypeVar("F", bound=Callable[..., Any])


def workflow(*, title: str, description: str = "") -> Callable[[F], F]:
    return _capability("workflow", title, description)


def fact(*, title: str, description: str = "") -> Callable[[F], F]:
    return _capability("fact", title, description)


def compute(*, title: str, description: str = "") -> Callable[[F], F]:
    return _capability("compute", title, description)


def _capability(kind: str, title: str, description: str) -> Callable[[F], F]:
    def decorate(fn: F) -> F:
        setattr(
            fn,
            "__kat_capability__",
            {
                "kind": kind,
                "name": fn.__name__,
                "title": title,
                "description": description,
            },
        )
        return fn

    return decorate
