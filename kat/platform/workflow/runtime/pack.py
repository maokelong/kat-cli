from __future__ import annotations

import builtins
from dataclasses import dataclass
import heapq
import importlib
import importlib.machinery
import importlib.util
import keyword
import os
from pathlib import Path
import sys
from types import ModuleType

import kat
from kat._workflow import _registration_count, _registrations_since

from .inspection import compile_declared_workflow


@dataclass(frozen=True)
class InspectPackResult:
    workflows: list[dict[str, object]]


def inspect_pack(pack_name: str, pack_path: str) -> InspectPackResult:
    if not isinstance(pack_name, str) or not pack_name:
        raise ValueError("PACK name must be a non-empty string")
    if not isinstance(pack_path, str):
        raise TypeError("PACK path must be a string")
    supplied = Path(pack_path)
    if not supplied.is_absolute():
        raise ValueError("PACK path must be canonical and absolute")
    root = supplied.resolve(strict=True)
    if root != supplied or not root.is_dir():
        raise ValueError("PACK path must identify its canonical directory")

    entries = _workflow_entries(root)
    _validate_module_conflicts(entries)
    _mount_current_pack(root)
    entry_module_names = {
        ".".join(("kat", "pack", "workflows", *segments)) for _, segments in entries
    }
    workflows: list[dict[str, object]] = []
    names: set[str] = set()
    for source, segments in entries:
        module_name = ".".join(("kat", "pack", "workflows", *segments))
        before = _registration_count()
        module = _import_entry(module_name, entry_module_names)
        actual_source = Path(module.__file__).resolve(strict=True) if module.__file__ else None
        if actual_source != source:
            raise ImportError(f"Workflow entry {source.relative_to(root).as_posix()} loaded from an unexpected module path")
        _reject_entry_imports(module, module_name, entry_module_names)
        registrations = _registrations_since(before)
        if len(registrations) != 1 or registrations[0].__module__ != module_name:
            relative = source.relative_to(root).as_posix()
            raise ValueError(f"Workflow entry {relative} must register exactly one Workflow defined by that module")
        compiled = compile_declared_workflow(registrations[0])
        name = compiled.interface["name"]
        if name in names:
            raise ValueError(f"duplicate Workflow name: {name}")
        names.add(name)
        workflows.append(compiled.interface)
    workflows.sort(key=lambda workflow: workflow["name"])
    return InspectPackResult(workflows=workflows)


def _import_entry(module_name: str, entry_module_names: set[str]) -> ModuleType:
    original_import = builtins.__import__
    original_import_module = importlib.import_module
    other_entries = entry_module_names - {module_name}
    hidden_entries = _hide_loaded_entries(other_entries)

    def guarded_import(
        name: str,
        globals: dict[str, object] | None = None,
        locals: dict[str, object] | None = None,
        fromlist: tuple[str, ...] | None = (),
        level: int = 0,
    ) -> ModuleType:
        requested = _requested_module_names(name, globals, fromlist, level)
        _reject_other_entry(module_name, other_entries, requested)
        return original_import(name, globals, locals, fromlist, level)

    def guarded_import_module(name: str, package: str | None = None) -> ModuleType:
        requested = importlib.util.resolve_name(name, package) if name.startswith(".") else name
        _reject_other_entry(module_name, other_entries, {requested})
        return original_import_module(name, package)

    builtins.__import__ = guarded_import
    importlib.import_module = guarded_import_module
    try:
        return original_import_module(module_name)
    finally:
        builtins.__import__ = original_import
        importlib.import_module = original_import_module
        for parent, attribute, entry in hidden_entries:
            setattr(parent, attribute, entry)


def _hide_loaded_entries(
    other_entries: set[str],
) -> list[tuple[ModuleType, str, ModuleType]]:
    hidden: list[tuple[ModuleType, str, ModuleType]] = []
    for entry_name in other_entries:
        parent_name, attribute = entry_name.rsplit(".", 1)
        parent = sys.modules.get(parent_name)
        entry = sys.modules.get(entry_name)
        if (
            isinstance(parent, ModuleType)
            and isinstance(entry, ModuleType)
            and getattr(parent, attribute, None) is entry
        ):
            hidden.append((parent, attribute, entry))
            delattr(parent, attribute)
    return hidden


def _reject_other_entry(
    module_name: str, other_entries: set[str], requested: set[str]
) -> None:
    for candidate in requested:
        if candidate != "kat.pack.workflows" and not candidate.startswith(
            "kat.pack.workflows."
        ):
            continue
        if any(
            candidate == entry or entry.startswith(candidate + ".")
            for entry in other_entries
        ):
            raise ImportError(f"Workflow entry {module_name} imports another Workflow entry")


def _requested_module_names(
    name: str,
    globals: dict[str, object] | None,
    fromlist: tuple[str, ...] | None,
    level: int,
) -> set[str]:
    try:
        if level:
            package = None if globals is None else globals.get("__package__")
            if type(package) is not str or not package:
                return set()
            base = importlib.util.resolve_name("." * level + name, package)
        else:
            base = name
    except (ImportError, ValueError):
        return set()
    requested = {base}
    requested.update(
        f"{base}.{member}"
        for member in fromlist or ()
        if type(member) is str and member and member != "*"
    )
    return requested


def _reject_entry_imports(
    module: ModuleType, module_name: str, entry_module_names: set[str]
) -> None:
    for value in vars(module).values():
        imported_module = value.__name__ if isinstance(value, ModuleType) else getattr(value, "__module__", None)
        if imported_module in entry_module_names and imported_module != module_name:
            raise ValueError(
                f"Workflow entry {module_name} imports another Workflow entry {imported_module}"
            )


def _workflow_entries(root: Path) -> list[tuple[Path, tuple[str, ...]]]:
    directory = root / "workflows"
    try:
        if not directory.exists():
            return []
        if not directory.is_dir():
            raise ValueError("PACK workflows path must be a directory")
        pending: list[tuple[str, Path, bool, bool]] = []
        entries: list[tuple[Path, tuple[str, ...]]] = []
        _enqueue_children(directory, directory, pending)
        while pending:
            _, path, is_directory, is_file = heapq.heappop(pending)
            if is_directory:
                _enqueue_children(directory, path, pending)
                continue
            if not is_file or path.suffix != ".py":
                continue
            relative = path.relative_to(directory)
            if path.name == "__init__.py":
                raise ValueError(f"Workflow initializer is not allowed: {(Path('workflows') / relative).as_posix()}")
            segments = (*relative.parts[:-1], path.stem)
            for segment in segments:
                if not segment.isidentifier() or keyword.iskeyword(segment):
                    raise ValueError(f"invalid Workflow module segment {segment!r} in {(Path('workflows') / relative).as_posix()}")
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(root) or not resolved.is_file():
                raise ValueError(f"Workflow entry is not an ordinary PACK file: {(Path('workflows') / relative).as_posix()}")
            entries.append((resolved, segments))
    except OSError as error:
        raise OSError(f"failed to scan PACK workflows directory {directory}") from error
    entries.sort(key=lambda item: item[0].relative_to(root).as_posix())
    return entries


def _enqueue_children(
    root: Path,
    directory: Path,
    pending: list[tuple[str, Path, bool, bool]],
) -> None:
    for child in sorted(os.scandir(directory), key=lambda entry: entry.name):
        path = Path(child.path)
        relative = path.relative_to(root).as_posix()
        heapq.heappush(
            pending,
            (
                relative,
                path,
                child.is_dir(follow_symlinks=False),
                child.is_file(follow_symlinks=False),
            ),
        )


def _validate_module_conflicts(entries: list[tuple[Path, tuple[str, ...]]]) -> None:
    by_segments = {segments: path for path, segments in entries}
    for segments, source in by_segments.items():
        for length in range(1, len(segments)):
            prefix = segments[:length]
            if prefix in by_segments:
                first = by_segments[prefix]
                raise ValueError(f"Workflow module/package conflict between {first} and {source}")


def _mount_current_pack(root: Path) -> None:
    if "kat.pack" in sys.modules or hasattr(kat, "pack"):
        raise RuntimeError("current PACK is already mounted")
    module = ModuleType("kat.pack")
    spec = importlib.machinery.ModuleSpec("kat.pack", loader=None, is_package=True)
    spec.submodule_search_locations = [str(root)]
    module.__spec__ = spec
    module.__package__ = "kat.pack"
    module.__path__ = [str(root)]
    sys.modules["kat.pack"] = module
    setattr(kat, "pack", module)
