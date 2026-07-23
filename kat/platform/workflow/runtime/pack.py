from __future__ import annotations

from dataclasses import dataclass
import heapq
import importlib
import importlib.machinery
import inspect
import keyword
import os
from pathlib import Path
import stat
import sys
from types import ModuleType

import kat
from kat._workflow import _registration_count, _registrations_since

from .inspection import WorkflowInterface, compile_declared_workflow


@dataclass(frozen=True)
class InspectPackRuntimeResult:
    workflows: list[WorkflowInterface]


class PackInspectionError(Exception):
    """A failure owned by the selected PACK production Interface."""


def inspect_pack(pack_name: str, pack_path: Path) -> InspectPackRuntimeResult:
    """Inspect the production Workflows of one selected PACK.

    ``pack_name`` preserves the business identity selected from its manifest.
    Only the canonical ``pack_path`` controls the checkout mounted as
    ``kat.pack``; its directory name does not define or verify PACK identity.
    """
    root = pack_path

    try:
        entries = _workflow_entries(root)
        _validate_module_conflicts(entries)
    except (OSError, ValueError) as error:
        raise PackInspectionError from error
    _mount_current_pack(root)
    entry_module_names = {
        ".".join(("kat", "pack", "workflows", *segments)) for _, segments in entries
    }
    workflows: list[WorkflowInterface] = []
    names: set[str] = set()
    for source, segments in entries:
        module_name = ".".join(("kat", "pack", "workflows", *segments))
        _clear_other_entries(entry_module_names, module_name)
        before = _registration_count()
        try:
            module = importlib.import_module(module_name)
        except (Exception, SystemExit) as error:
            raise PackInspectionError from error
        try:
            module_file = vars(module).get("__file__")
            actual_source = (
                Path(module_file).resolve(strict=True)
                if type(module_file) is str
                else None
            )
        except (OSError, RuntimeError) as error:
            raise PackInspectionError from error
        if actual_source != source:
            raise PackInspectionError(
                f"Workflow entry {source.relative_to(root).as_posix()} loaded from an unexpected module path"
            )
        registrations = _registrations_since(before)
        registration = registrations[0] if len(registrations) == 1 else None
        registration_module = (
            object.__getattribute__(registration, "__module__")
            if inspect.isfunction(registration)
            else None
        )
        if type(registration_module) is not str or registration_module != module_name:
            relative = source.relative_to(root).as_posix()
            raise PackInspectionError(
                f"Workflow entry {relative} must register exactly one Workflow defined by that module"
            )
        try:
            compiled = compile_declared_workflow(registrations[0])
        except ValueError as error:
            raise PackInspectionError from error
        name = compiled.interface["name"]
        if name in names:
            raise PackInspectionError(f"duplicate Workflow name: {name}")
        names.add(name)
        workflows.append(compiled.interface)
    workflows.sort(key=lambda workflow: workflow["name"])
    return InspectPackRuntimeResult(workflows=workflows)

def _clear_other_entries(entry_names: set[str], current: str) -> None:
    for entry_name in entry_names - {current}:
        parent_name, attribute = entry_name.rsplit(".", 1)
        parent = sys.modules.get(parent_name)
        entry = sys.modules.pop(entry_name, None)
        if (
            isinstance(parent, ModuleType)
            and isinstance(entry, ModuleType)
            and getattr(parent, attribute, None) is entry
        ):
            delattr(parent, attribute)


def _workflow_entries(root: Path) -> list[tuple[Path, tuple[str, ...]]]:
    directory = root / "workflows"
    try:
        try:
            metadata = directory.lstat()
        except FileNotFoundError:
            return []
        if stat.S_ISLNK(metadata.st_mode):
            metadata = directory.stat()
        if not stat.S_ISDIR(metadata.st_mode):
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
