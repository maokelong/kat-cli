from __future__ import annotations

import importlib.util
import inspect
import sys
from pathlib import Path
from types import ModuleType
from typing import Any


KINDS = ("workflow", "fact", "compute")


def load_pack_modules(pack_root: Path) -> list[ModuleType]:
    pack_root = pack_root.resolve()
    module_files = sorted(path for path in pack_root.rglob("*.py") if path.name != "__init__.py")
    modules: list[ModuleType] = []
    sys.path.insert(0, str(pack_root))
    try:
        for index, path in enumerate(module_files):
            module_name = f"kat_pack_{index}_{path.stem}"
            spec = importlib.util.spec_from_file_location(module_name, path)
            if spec is None or spec.loader is None:
                raise RuntimeError(f"failed to load pack module spec: {path}")
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            modules.append(module)
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


def find_workflow(pack_root: Path, workflow_name: str):
    for module in load_pack_modules(pack_root):
        for value in _iter_module_capabilities(module):
            metadata = getattr(value, "__kat_capability__", None)
            if metadata and metadata["kind"] == "workflow" and metadata["name"] == workflow_name:
                return value
    raise KeyError(f"workflow not found: {workflow_name}")


def _iter_module_capabilities(module: ModuleType):
    for _, value in inspect.getmembers(module, inspect.isfunction):
        if value.__module__ != module.__name__:
            continue
        yield value
