from __future__ import annotations

from dataclasses import dataclass, field
import hashlib
import heapq
import importlib
import importlib.machinery
import importlib.util
import inspect
import keyword
import multiprocessing
from multiprocessing.connection import Connection
import os
from pathlib import Path
import stat
import sys
from types import ModuleType
from typing import Literal, cast

import kat
from kat._source import (
    _registration_count as _source_registration_count,
    _registrations_since as _source_registrations_since,
)
from kat._workflow import (
    _registration_count as _workflow_registration_count,
    _registrations_since as _workflow_registrations_since,
)

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .inspection import (
    CompiledSource,
    CompiledWorkflow,
    SourceInputInterface,
    WorkflowInputInterface,
    compile_declared_source,
    compile_declared_workflow,
)


type _EntryKind = Literal["source", "workflow"]
type _EntryInterface = SourceInputInterface | WorkflowInputInterface
type _CompiledEntry = CompiledSource | CompiledWorkflow


_PUBLIC_WORKFLOW_MODULE_ROOT = "kat.pack"


@dataclass(frozen=True)
class InspectPackRuntimeResult:
    source_guide: str | None
    sources: list[SourceInputInterface]
    workflows: list[WorkflowInputInterface]


@dataclass(frozen=True)
class PackLoadingProfile:
    scan_sources: bool
    scan_workflows: bool
    read_source_guide: bool


INSPECTION_PROFILE = PackLoadingProfile(True, True, True)
TEST_PROFILE = PackLoadingProfile(True, True, True)
SOURCE_OPERATION_PROFILE = PackLoadingProfile(True, False, True)
SOURCE_RESOLUTION_PROFILE = PackLoadingProfile(True, False, False)
RUN_PROFILE = PackLoadingProfile(True, True, False)


class PackInspectionError(Exception):
    """A failure owned by the selected PACK production Interface."""

    def __init__(self, diagnostic: RuntimeDiagnostic) -> None:
        super().__init__(diagnostic["message"])
        self.diagnostic = diagnostic


class _PackInspectionWorkerError(RuntimeError):
    """The private inspection worker did not complete its Runtime protocol."""


@dataclass(frozen=True)
class _EntrySuccess:
    interface: _EntryInterface


@dataclass(frozen=True)
class _EntryFailure:
    diagnostic: RuntimeDiagnostic


type _EntryOutcome = _EntrySuccess | _EntryFailure


@dataclass(frozen=True)
class _InspectedEntry:
    source: Path
    module_name: str
    interface: _EntryInterface


@dataclass(frozen=True)
class ProductionPack:
    """One validated production PACK exposed through inspection and selection."""

    name: str
    root: Path
    workflow_module_root: str
    source_module_root: str
    source_guide: str | None
    source_entries: tuple[_InspectedEntry, ...]
    workflow_entries: tuple[_InspectedEntry, ...]
    _loaded_sources: dict[str, CompiledSource] = field(
        default_factory=dict,
        compare=False,
        repr=False,
    )

    @classmethod
    def open(
        cls,
        selected_pack_name: str,
        pack_path: Path,
        *,
        profile: PackLoadingProfile = RUN_PROFILE,
    ) -> ProductionPack:
        if not selected_pack_name:
            raise ValueError("PACK name must be a non-empty string")
        root = pack_path
        try:
            source_files = _source_entries(root) if profile.scan_sources else []
            workflow_files = _workflow_entries(root) if profile.scan_workflows else []
            _validate_module_conflicts(source_files, "Source")
            _validate_module_conflicts(workflow_files, "Workflow")
            source_guide = (
                _read_source_guide(root, has_sources=bool(source_files))
                if profile.read_source_guide
                else None
            )
        except (OSError, UnicodeError, ValueError) as error:
            raise _pack_failure(error, root) from error

        effective_source_module_root = private_source_module_root(root)
        sources = _inspect_entries(
            selected_pack_name,
            root,
            source_files,
            kind="source",
            module_root=effective_source_module_root,
        )
        workflows = _inspect_entries(
            selected_pack_name,
            root,
            workflow_files,
            kind="workflow",
            module_root=_PUBLIC_WORKFLOW_MODULE_ROOT,
        )
        return cls(
            name=selected_pack_name,
            root=root,
            workflow_module_root=_PUBLIC_WORKFLOW_MODULE_ROOT,
            source_module_root=effective_source_module_root,
            source_guide=source_guide,
            source_entries=sources,
            workflow_entries=workflows,
        )

    def inspect(self) -> InspectPackRuntimeResult:
        return InspectPackRuntimeResult(
            source_guide=self.source_guide,
            sources=[
                cast(SourceInputInterface, entry.interface)
                for entry in self.source_entries
            ],
            workflows=[
                cast(WorkflowInputInterface, entry.interface)
                for entry in self.workflow_entries
            ],
        )

    def load(self, workflow_name: str) -> CompiledWorkflow:
        entry = _select_entry(self.workflow_entries, workflow_name, "Workflow")
        _mount_pack(self.root, self.workflow_module_root)
        compiled = cast(
            CompiledWorkflow,
            _load_entry(
                self.root,
                entry.source,
                entry.module_name,
                "workflow",
                self.workflow_module_root,
            ),
        )
        _require_unchanged(self.root, entry, compiled.interface, "Workflow")
        return compiled

    def load_source(self, source_name: str) -> CompiledSource:
        cached = self._loaded_sources.get(source_name)
        if cached is not None:
            return cached
        entry = _select_entry(self.source_entries, source_name, "Source")
        _mount_pack(self.root, self.source_module_root)
        compiled = cast(
            CompiledSource,
            _load_entry(
                self.root,
                entry.source,
                entry.module_name,
                "source",
                self.source_module_root,
            ),
        )
        _require_unchanged(self.root, entry, compiled.interface, "Source")
        self._loaded_sources[source_name] = compiled
        return compiled

    def load_all(self) -> dict[str, CompiledWorkflow]:
        _mount_pack(self.root, self.workflow_module_root)
        workflows: dict[str, CompiledWorkflow] = {}
        for entry in self.workflow_entries:
            compiled = cast(
                CompiledWorkflow,
                _load_entry(
                    self.root,
                    entry.source,
                    entry.module_name,
                    "workflow",
                    self.workflow_module_root,
                ),
            )
            _require_unchanged(self.root, entry, compiled.interface, "Workflow")
            workflows[compiled.interface["name"]] = compiled
        return workflows

    def load_all_sources(self) -> dict[str, CompiledSource]:
        _mount_pack(self.root, self.source_module_root)
        sources: dict[str, CompiledSource] = {}
        for entry in self.source_entries:
            compiled = cast(
                CompiledSource,
                _load_entry(
                    self.root,
                    entry.source,
                    entry.module_name,
                    "source",
                    self.source_module_root,
                ),
            )
            _require_unchanged(self.root, entry, compiled.interface, "Source")
            sources[compiled.interface["name"]] = compiled
        return sources


def inspect_pack(selected_pack_name: str, pack_path: Path) -> InspectPackRuntimeResult:
    """Inspect the Sources and Workflows of one selected PACK."""
    return ProductionPack.open(
        selected_pack_name,
        pack_path,
        profile=INSPECTION_PROFILE,
    ).inspect()


def _inspect_entries(
    selected_pack_name: str,
    root: Path,
    discovered: list[tuple[Path, tuple[str, ...]]],
    *,
    kind: _EntryKind,
    module_root: str,
) -> tuple[_InspectedEntry, ...]:
    plural = "sources" if kind == "source" else "workflows"
    owner = _owner(kind)
    entries: list[_InspectedEntry] = []
    names: set[str] = set()
    for source, segments in discovered:
        module_name = ".".join((module_root, plural, *segments))
        outcome = _inspect_entry_isolated(
            selected_pack_name,
            root,
            source,
            module_name,
            kind,
            module_root,
        )
        if isinstance(outcome, _EntryFailure):
            raise PackInspectionError(outcome.diagnostic)
        name = outcome.interface["name"]
        if name in names:
            raise _pack_failure(ValueError(f"duplicate {owner} name: {name}"), root)
        names.add(name)
        entries.append(
            _InspectedEntry(
                source=source,
                module_name=module_name,
                interface=outcome.interface,
            )
        )
    entries.sort(key=lambda entry: entry.interface["name"])
    return tuple(entries)


def _inspect_entry_isolated(
    selected_pack_name: str,
    root: Path,
    source: Path,
    module_name: str,
    kind: _EntryKind,
    module_root: str,
) -> _EntryOutcome:
    owner = _owner(kind)
    context = multiprocessing.get_context("spawn")
    receive, send = context.Pipe(duplex=False)
    process = context.Process(
        target=_inspect_entry_worker,
        args=(send, root, source, module_name, kind, module_root),
        name=f"kat-inspect-{selected_pack_name}:{module_name}",
    )
    started = False
    try:
        process.start()
        started = True
        send.close()
        try:
            outcome = receive.recv()
        except EOFError as error:
            process.join()
            raise _PackInspectionWorkerError(
                f"{owner} inspection worker exited without a result: {module_name}"
            ) from error
        process.join()
        if process.exitcode != 0:
            raise _PackInspectionWorkerError(
                f"{owner} inspection worker exited with code {process.exitcode}: {module_name}"
            )
        if not isinstance(outcome, (_EntrySuccess, _EntryFailure)):
            raise _PackInspectionWorkerError(
                f"{owner} inspection worker returned an invalid result: {module_name}"
            )
        return outcome
    finally:
        receive.close()
        send.close()
        if started:
            if process.is_alive():
                process.terminate()
            process.join()
        process.close()


def _inspect_entry_worker(
    connection: Connection,
    root: Path,
    source: Path,
    module_name: str,
    kind: _EntryKind,
    module_root: str,
) -> None:
    try:
        _mount_pack(root, module_root)
        outcome = _inspect_entry(root, source, module_name, kind, module_root)
        connection.send(outcome)
    finally:
        connection.close()


def _inspect_entry(
    root: Path,
    source: Path,
    module_name: str,
    kind: _EntryKind,
    module_root: str,
) -> _EntryOutcome:
    try:
        compiled = _load_entry(root, source, module_name, kind, module_root)
    except (Exception, SystemExit) as error:
        return _EntryFailure(_pack_diagnostic(error, root))
    return _EntrySuccess(compiled.interface)


def _load_entry(
    root: Path,
    source: Path,
    module_name: str,
    kind: _EntryKind,
    module_root: str,
) -> _CompiledEntry:
    source_before = _source_registration_count()
    workflow_before = _workflow_registration_count()
    module = importlib.import_module(module_name)
    module_file = vars(module).get("__file__")
    actual_source = (
        Path(module_file).resolve(strict=True) if type(module_file) is str else None
    )
    owner = _owner(kind)
    if actual_source != source:
        raise ValueError(
            f"{owner} entry {source.relative_to(root).as_posix()} "
            "loaded from an unexpected module path"
        )

    source_registrations = _source_registrations_since(source_before)
    workflow_registrations = _workflow_registrations_since(workflow_before)
    registrations = (
        source_registrations if kind == "source" else workflow_registrations
    )
    unrelated = (
        workflow_registrations if kind == "source" else source_registrations
    )
    registration = registrations[0] if len(registrations) == 1 else None
    registration_module = (
        object.__getattribute__(registration, "__module__")
        if inspect.isfunction(registration)
        else None
    )
    if (
        unrelated
        or type(registration_module) is not str
        or registration_module != module_name
    ):
        relative = source.relative_to(root).as_posix()
        raise ValueError(
            f"{owner} entry {relative} must register exactly one {owner} "
            "defined by that module"
        )
    if kind == "source":
        return compile_declared_source(registration)
    return compile_declared_workflow(registration)


def _select_entry(
    entries: tuple[_InspectedEntry, ...],
    name: str,
    owner: str,
) -> _InspectedEntry:
    entry = next((entry for entry in entries if entry.interface["name"] == name), None)
    if entry is None:
        raise ValueError(f"{owner} {name!r} was not found in the selected PACK")
    return entry


def _require_unchanged(
    root: Path,
    entry: _InspectedEntry,
    interface: _EntryInterface,
    owner: str,
) -> None:
    if interface != entry.interface:
        raise ValueError(
            f"{owner} entry {entry.source.relative_to(root).as_posix()} "
            "changed between inspection and execution loading"
        )


def _pack_failure(error: BaseException, root: Path) -> PackInspectionError:
    return PackInspectionError(_pack_diagnostic(error, root))


def _pack_diagnostic(error: BaseException, root: Path) -> RuntimeDiagnostic:
    return diagnostic_from_exception(
        error,
        root,
        message="PACK inspection failed",
        help="Correct the PACK production Interface and retry inspection",
    )


def _source_entries(root: Path) -> list[tuple[Path, tuple[str, ...]]]:
    return _entries(root, "sources", "Source")


def _workflow_entries(root: Path) -> list[tuple[Path, tuple[str, ...]]]:
    return _entries(root, "workflows", "Workflow")


def _entries(
    root: Path,
    directory_name: str,
    owner: str,
) -> list[tuple[Path, tuple[str, ...]]]:
    directory = root / directory_name
    try:
        try:
            metadata = directory.lstat()
        except FileNotFoundError:
            return []
        if not stat.S_ISDIR(metadata.st_mode):
            return []
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
            display_path = (Path(directory_name) / relative).as_posix()
            if path.name == "__init__.py":
                raise ValueError(f"{owner} initializer is not allowed: {display_path}")
            segments = (*relative.parts[:-1], path.stem)
            for segment in segments:
                if not segment.isidentifier() or keyword.iskeyword(segment):
                    raise ValueError(
                        f"invalid {owner} module segment {segment!r} in {display_path}"
                    )
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(root) or not resolved.is_file():
                raise ValueError(
                    f"{owner} entry is not an ordinary PACK file: {display_path}"
                )
            entries.append((resolved, segments))
    except OSError as error:
        raise OSError(f"failed to scan PACK {directory_name} directory {directory}") from error
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


def _validate_module_conflicts(
    entries: list[tuple[Path, tuple[str, ...]]],
    owner: str,
) -> None:
    by_segments = {segments: path for path, segments in entries}
    for segments, source in by_segments.items():
        for length in range(1, len(segments)):
            prefix = segments[:length]
            if prefix in by_segments:
                first = by_segments[prefix]
                raise ValueError(
                    f"{owner} module/package conflict between {first} and {source}"
                )


def _read_source_guide(root: Path, *, has_sources: bool) -> str | None:
    path = root / "SOURCES.md"
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        if has_sources:
            raise ValueError("PACK with Sources must provide SOURCES.md") from None
        return None
    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError("PACK SOURCES.md must be an ordinary file")
    try:
        return path.read_bytes().decode("utf-8", errors="strict")
    except UnicodeError as error:
        raise ValueError("PACK SOURCES.md must contain valid UTF-8") from error
    except OSError as error:
        raise OSError("failed to read PACK SOURCES.md") from error


def _owner(kind: _EntryKind) -> str:
    return "Source" if kind == "source" else "Workflow"


def private_source_module_root(root: Path) -> str:
    canonical_root = root.resolve(strict=True)
    digest = hashlib.sha256(
        os.fsencode(os.path.normcase(str(canonical_root)))
    ).hexdigest()
    return f"_kat_source_pack_{digest}"


def _mount_pack(root: Path, module_root: str) -> None:
    existing = sys.modules.get(module_root)
    if existing is not None:
        if vars(existing).get("__kat_pack_root__") == root:
            return
        raise RuntimeError(f"a different PACK is already mounted as {module_root}")
    public = module_root == "kat.pack"
    if public and hasattr(kat, "pack"):
        raise RuntimeError("current PACK is already mounted inconsistently")
    module = ModuleType(module_root)
    spec = importlib.machinery.ModuleSpec(module_root, loader=None, is_package=True)
    spec.submodule_search_locations = [str(root)]
    module.__spec__ = spec
    module.__package__ = module_root
    module.__path__ = [str(root)]
    module.__kat_pack_root__ = root
    sys.modules[module_root] = module
    if public:
        setattr(kat, "pack", module)
