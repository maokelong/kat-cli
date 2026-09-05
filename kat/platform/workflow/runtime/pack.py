from __future__ import annotations

from dataclasses import dataclass
import heapq
import importlib
import importlib.machinery
import inspect
import keyword
import multiprocessing
from multiprocessing.connection import Connection
import os
from pathlib import Path
import stat
import sys
from types import ModuleType
from typing import TypedDict

import kat
from kat._workflow import _registration_count, _registrations_since

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .inspection import (
    CompiledWorkflow,
    WorkflowInputInterface,
    WorkflowParameter,
    compile_declared_workflow,
)
from .knowledge import read_guide


class WorkflowSummary(TypedDict):
    name: str
    description: str


class WorkflowDetail(TypedDict):
    name: str
    description: str
    parameters: list[WorkflowParameter]
    guide: str | None


@dataclass(frozen=True)
class InspectWorkflowsRuntimeResult:
    workflows: list[WorkflowSummary]


@dataclass(frozen=True)
class InspectWorkflowDetailRuntimeResult:
    workflow: WorkflowDetail


type InspectWorkflowRuntimeResult = (
    InspectWorkflowsRuntimeResult | InspectWorkflowDetailRuntimeResult
)


class PackInspectionError(Exception):
    """A failure owned by the selected PACK production Interface."""

    def __init__(self, diagnostic: RuntimeDiagnostic) -> None:
        super().__init__(diagnostic["message"])
        self.diagnostic = diagnostic


class _PackInspectionWorkerError(RuntimeError):
    """The private inspection worker did not complete its Runtime protocol."""


@dataclass(frozen=True)
class _EntrySuccess:
    interface: WorkflowInputInterface
    guide_ref: str | None


@dataclass(frozen=True)
class _EntryFailure:
    diagnostic: RuntimeDiagnostic


type _EntryOutcome = _EntrySuccess | _EntryFailure


@dataclass(frozen=True)
class _InspectedEntry:
    source: Path
    module_name: str
    interface: WorkflowInputInterface
    guide_ref: str | None
    guide: str | None


@dataclass(frozen=True)
class ProductionPack:
    """One validated production PACK exposed through inspection and selection."""

    name: str
    root: Path
    entries: tuple[_InspectedEntry, ...]

    @classmethod
    def open(cls, selected_pack_name: str, pack_path: Path) -> ProductionPack:
        if not selected_pack_name:
            raise ValueError("PACK name must be a non-empty string")
        root = pack_path
        try:
            discovered = _workflow_entries(root)
            _validate_module_conflicts(discovered)
        except (OSError, ValueError) as error:
            raise _pack_failure(error, root) from error

        entries: list[_InspectedEntry] = []
        names: set[str] = set()
        for source, segments in discovered:
            module_name = ".".join(("kat", "pack", "workflows", *segments))
            outcome = _inspect_entry_isolated(
                selected_pack_name,
                root,
                source,
                module_name,
            )
            if isinstance(outcome, _EntryFailure):
                raise PackInspectionError(outcome.diagnostic)
            name = outcome.interface["name"]
            if name in names:
                error = ValueError(f"duplicate Workflow name: {name}")
                raise _pack_failure(error, root)
            names.add(name)
            try:
                guide = (
                    None
                    if outcome.guide_ref is None
                    else read_guide(
                        root,
                        outcome.guide_ref,
                        declaration=f"Workflow {name!r}",
                        category="workflows",
                    )
                )
            except (OSError, ValueError) as error:
                raise _pack_failure(error, root) from error
            entries.append(
                _InspectedEntry(
                    source=source,
                    module_name=module_name,
                    interface=outcome.interface,
                    guide_ref=outcome.guide_ref,
                    guide=guide,
                )
            )
        entries.sort(key=lambda entry: entry.interface["name"])
        return cls(name=selected_pack_name, root=root, entries=tuple(entries))

    def inspect(self, workflow_name: str | None = None) -> InspectWorkflowRuntimeResult:
        if workflow_name is None:
            return InspectWorkflowsRuntimeResult(
                workflows=[
                    {
                        "name": entry.interface["name"],
                        "description": entry.interface["description"],
                    }
                    for entry in self.entries
                ]
            )
        entry = next(
            (
                entry
                for entry in self.entries
                if entry.interface["name"] == workflow_name
            ),
            None,
        )
        if entry is None:
            raise _pack_failure(
                ValueError(
                    f"Workflow {workflow_name!r} was not found in the selected PACK"
                ),
                self.root,
            )
        return InspectWorkflowDetailRuntimeResult(
            workflow={
                "name": entry.interface["name"],
                "description": entry.interface["description"],
                "parameters": list(entry.interface["parameters"]),
                "guide": entry.guide,
            }
        )

    def load(self, workflow_name: str) -> CompiledWorkflow:
        entry = next(
            (
                entry
                for entry in self.entries
                if entry.interface["name"] == workflow_name
            ),
            None,
        )
        if entry is None:
            raise ValueError(
                f"Workflow {workflow_name!r} was not found in the selected PACK"
            )
        _mount_current_pack(self.root)
        compiled = _load_entry(self.root, entry.source, entry.module_name)
        if (
            compiled.interface != entry.interface
            or compiled.guide_ref != entry.guide_ref
        ):
            raise ValueError(
                f"Workflow entry {entry.source.relative_to(self.root).as_posix()} "
                "changed between inspection and execution loading"
            )
        return compiled

    def mount_for_tests(self) -> None:
        # helper 单测属于 pytest；实际 Workflow 在独立 Runtime 内正式加载。
        _mount_current_pack(self.root)


def inspect_workflow(
    selected_pack_name: str,
    pack_path: Path,
    workflow_name: str | None = None,
) -> InspectWorkflowRuntimeResult:
    """Inspect the production Workflows of one selected PACK.

    The CLI has already selected ``selected_pack_name`` and its canonical
    checkout. Runtime mounts that checkout as ``kat.pack``; it does not bind or
    verify PACK identity from the checkout directory name.
    """
    return ProductionPack.open(selected_pack_name, pack_path).inspect(workflow_name)


def _inspect_entry_isolated(
    selected_pack_name: str,
    root: Path,
    source: Path,
    module_name: str,
) -> _EntryOutcome:
    context = multiprocessing.get_context("spawn")
    receive, send = context.Pipe(duplex=False)
    process = context.Process(
        target=_inspect_entry_worker,
        args=(send, root, source, module_name),
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
                f"Workflow inspection worker exited without a result: {module_name}"
            ) from error
        process.join()
        if process.exitcode != 0:
            raise _PackInspectionWorkerError(
                f"Workflow inspection worker exited with code {process.exitcode}: {module_name}"
            )
        if not isinstance(outcome, (_EntrySuccess, _EntryFailure)):
            raise _PackInspectionWorkerError(
                f"Workflow inspection worker returned an invalid result: {module_name}"
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
) -> None:
    try:
        _mount_current_pack(root)
        outcome = _inspect_entry(root, source, module_name)
        connection.send(outcome)
    finally:
        connection.close()


def _inspect_entry(root: Path, source: Path, module_name: str) -> _EntryOutcome:
    try:
        compiled = _load_entry(root, source, module_name)
    except (Exception, SystemExit) as error:
        return _EntryFailure(_pack_diagnostic(error, root))
    return _EntrySuccess(compiled.interface, compiled.guide_ref)


def _load_entry(root: Path, source: Path, module_name: str) -> CompiledWorkflow:
    before = _registration_count()
    module = importlib.import_module(module_name)
    try:
        module_file = vars(module).get("__file__")
        actual_source = (
            Path(module_file).resolve(strict=True) if type(module_file) is str else None
        )
    except (OSError, RuntimeError):
        raise
    if actual_source != source:
        raise ValueError(
            f"Workflow entry {source.relative_to(root).as_posix()} "
            "loaded from an unexpected module path"
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
        raise ValueError(
            f"Workflow entry {relative} must register exactly one Workflow "
            "defined by that module"
        )
    return compile_declared_workflow(registration)


def _pack_failure(error: BaseException, root: Path) -> PackInspectionError:
    return PackInspectionError(_pack_diagnostic(error, root))


def _pack_diagnostic(error: BaseException, root: Path) -> RuntimeDiagnostic:
    return diagnostic_from_exception(
        error,
        root,
        message="PACK inspection failed",
        help="Correct the PACK production Interface and retry inspection",
    )


def _workflow_entries(root: Path) -> list[tuple[Path, tuple[str, ...]]]:
    directory = root / "workflows"
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
