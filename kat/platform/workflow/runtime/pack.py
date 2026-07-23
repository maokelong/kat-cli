from __future__ import annotations

import builtins
from dataclasses import dataclass
import heapq
import importlib
import importlib.machinery
import importlib.util
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


class _EntryImportGuard:
    def __init__(self, entry_module_names: set[str]) -> None:
        self._entry_module_names = entry_module_names
        self._active_entry: str | None = None
        self._original_import = builtins.__import__
        self._original_importlib_import = importlib.__import__
        self._original_import_module = importlib.import_module

    def import_entry(self, module_name: str) -> ModuleType:
        if self._active_entry is not None:
            raise RuntimeError("Workflow entry import is already active")
        other_entries = self._entry_module_names - {module_name}
        hidden_entries = _hide_loaded_entries(other_entries)
        self._active_entry = module_name
        builtins.__import__ = self._guarded_import
        importlib.__import__ = self._guarded_import
        importlib.import_module = self._guarded_import_module
        try:
            return self._original_import_module(module_name)
        finally:
            builtins.__import__ = self._original_import
            importlib.__import__ = self._original_importlib_import
            importlib.import_module = self._original_import_module
            self._active_entry = None
            for parent, attribute, entry in hidden_entries:
                setattr(parent, attribute, entry)

    def _guarded_import(
        self,
        name: str,
        globals: dict[str, object] | None = None,
        locals: dict[str, object] | None = None,
        fromlist: tuple[str, ...] | None = (),
        level: int = 0,
    ) -> ModuleType:
        requested = _requested_module_names(name, globals, fromlist, level)
        self._reject_other_entry(requested)
        return self._original_import(name, globals, locals, fromlist, level)

    def _guarded_import_module(self, name: str, package: str | None = None) -> ModuleType:
        requested = importlib.util.resolve_name(name, package) if name.startswith(".") else name
        self._reject_other_entry({requested})
        return self._original_import_module(name, package)

    def _reject_other_entry(self, requested: set[str]) -> None:
        if self._active_entry is None:
            return
        _reject_other_entry(
            self._active_entry,
            self._entry_module_names - {self._active_entry},
            requested,
        )


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
    import_guard = _EntryImportGuard(entry_module_names)
    workflows: list[WorkflowInterface] = []
    names: set[str] = set()
    for source, segments in entries:
        module_name = ".".join(("kat", "pack", "workflows", *segments))
        before = _registration_count()
        try:
            module = import_guard.import_entry(module_name)
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
        try:
            _reject_entry_imports(module, module_name, entry_module_names)
        except ValueError as error:
            raise PackInspectionError from error
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
        if inspect.isfunction(value):
            imported_module = object.__getattribute__(value, "__module__")
        else:
            attribute = "__name__" if isinstance(value, ModuleType) else "__module__"
            imported_module = inspect.getattr_static(value, attribute, None)
        if type(imported_module) is not str:
            continue
        if imported_module in entry_module_names and imported_module != module_name:
            raise ValueError(
                f"Workflow entry {module_name} imports another Workflow entry {imported_module}"
            )


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
