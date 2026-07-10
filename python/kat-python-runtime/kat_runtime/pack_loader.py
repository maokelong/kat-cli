from __future__ import annotations

import hashlib
import importlib
import importlib.util
import inspect
import sys
from pathlib import Path
from types import ModuleType
from typing import Any


KINDS = ("workflow", "fact", "compute")


def load_pack_modules(pack_root: Path) -> list[ModuleType]:
    pack_root = pack_root.resolve()
    package_name = _package_name(pack_root)
    _clear_loaded_package(package_name)
    package = _load_root_package(package_name, pack_root)
    module_files = sorted(path for path in pack_root.rglob("*.py") if path.name != "__init__.py")
    modules: list[ModuleType] = [package]
    sys.path.insert(0, str(pack_root))
    try:
        for path in module_files:
            relative_name = ".".join(path.relative_to(pack_root).with_suffix("").parts)
            modules.append(importlib.import_module(f".{relative_name}", package_name))
    finally:
        sys.path.remove(str(pack_root))
    return modules


def discover_pack(pack_root: Path) -> dict[str, list[dict[str, Any]]]:
    manifest: dict[str, list[dict[str, Any]]] = {
        "workflows": [],
        "facts": [],
        "computes": [],
    }
    for module in load_pack_modules(pack_root):
        for value in _iter_module_capabilities(module):
            metadata = getattr(value, "__kat_capability__", None)
            if not metadata:
                continue
            kind = metadata["kind"]
            if kind not in KINDS:
                continue
            manifest[f"{kind}s"].append(
                {
                    "name": metadata["name"],
                    "title": metadata["title"],
                    "description": metadata["description"],
                    "module": module.__name__,
                    "signature": str(inspect.signature(value)),
                }
            )
    for capabilities in manifest.values():
        capabilities.sort(key=lambda item: item["name"])
    return manifest


def find_workflow(modules: list[ModuleType], workflow_name: str):
    matches = []
    for module in modules:
        for value in _iter_module_capabilities(module):
            metadata = getattr(value, "__kat_capability__", None)
            if (
                metadata
                and metadata["kind"] == "workflow"
                and metadata["name"] == workflow_name
            ):
                matches.append(value)
    if not matches:
        raise KeyError(f"workflow not found: {workflow_name}")
    if len(matches) > 1:
        raise ValueError(f"workflow is ambiguous: {workflow_name}")
    return matches[0]


def _iter_module_capabilities(module: ModuleType):
    for _, value in inspect.getmembers(module, inspect.isfunction):
        if value.__module__ != module.__name__:
            continue
        yield value


def _package_name(pack_root: Path) -> str:
    digest = hashlib.sha1(str(pack_root).encode("utf-8")).hexdigest()[:12]
    return f"kat_pack_{digest}"


def _clear_loaded_package(package_name: str) -> None:
    for name in list(sys.modules):
        if name == package_name or name.startswith(f"{package_name}."):
            del sys.modules[name]


def _load_root_package(package_name: str, pack_root: Path) -> ModuleType:
    init_file = pack_root / "__init__.py"
    if init_file.exists():
        spec = importlib.util.spec_from_file_location(
            package_name,
            init_file,
            submodule_search_locations=[str(pack_root)],
        )
        if spec is None or spec.loader is None:
            raise RuntimeError(f"failed to load pack package spec: {pack_root}")
        module = importlib.util.module_from_spec(spec)
        sys.modules[package_name] = module
        spec.loader.exec_module(module)
        return module

    module = ModuleType(package_name)
    module.__file__ = str(init_file)
    module.__package__ = package_name
    module.__path__ = [str(pack_root)]  # type: ignore[attr-defined]
    sys.modules[package_name] = module
    return module
