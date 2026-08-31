from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class _ProviderDeclaration:
    name: str
    description: str
    guide: str


def provider(
    *,
    name: str,
    description: str,
    guide: str,
) -> Callable[[type[Any]], type[Any]]:
    """Attach KAT inspection metadata to a user-defined Provider class.

    The decorator returns the original class unchanged. It defines no Provider
    base class, lifecycle, query API, or registration side effect.
    """
    if type(name) is not str:
        raise TypeError("Provider name must be a string")
    if not name.strip():
        raise ValueError("Provider name must not be empty")
    if type(description) is not str:
        raise TypeError("Provider description must be a string")
    if not description.strip():
        raise ValueError("Provider description must not be empty")
    if type(guide) is not str:
        raise TypeError("Provider guide must be a string")
    if not guide.strip():
        raise ValueError("Provider guide must not be empty")
    declaration = _ProviderDeclaration(
        name=name,
        description=description.strip(),
        guide=guide.strip(),
    )

    def decorate(provider_class: type[Any]) -> type[Any]:
        if not isinstance(provider_class, type):
            raise TypeError("Provider must be a class")
        if "__kat_provider__" in vars(provider_class):
            raise ValueError("a class can declare only one Provider")
        setattr(provider_class, "__kat_provider__", declaration)
        return provider_class

    return decorate
