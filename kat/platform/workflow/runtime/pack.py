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

import kat
from kat._workflow import _registration_count, _registrations_since

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .inspection import WorkflowInterface, compile_declared_workflow


@dataclass(frozen=True)
class InspectPackRuntimeResult:
    workflows: list[WorkflowInterface]


class PackInspectionError(Exception):
    """A failure owned by the selected PACK production Interface."""

    def __init__(self, diagnostic: RuntimeDiagnostic) -> None:
        super().__init__(diagnostic["message"])
        self.diagnostic = diagnostic


@dataclass(frozen=True)
class _EntrySuccess:
    interface: WorkflowInterface


@dataclass(frozen=True)
class _EntryFailure:
    diagnostic: RuntimeDiagnostic


type _EntryOutcome = _EntrySuccess | _EntryFailure


def inspect_pack(selected_pack_name: str, pack_path: Path) -> InspectPackRuntimeResult:
    """Inspect the production Workflows of one selected PACK.

    The CLI has already selected ``selected_pack_name`` and its canonical
    checkout. Runtime mounts that checkout as ``kat.pack``; it does not bind or
    verify PACK identity from the checkout directory name.
    """
    root = pack_path

    try:
        entries = _workflow_entries(root)
        _validate_module_conflicts(entries)
    except (OSError, ValueError) as error:
        raise _pack_failure(error, root) from error
    workflows: list[WorkflowInterface] = []
    names: set[str] = set()
    for source, segments in entries:
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
        workflows.append(outcome.interface)
    workflows.sort(key=lambda workflow: workflow["name"])
    return InspectPackRuntimeResult(workflows=workflows)


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
            raise RuntimeError(
                f"Workflow inspection worker exited without a result: {module_name}"
            ) from error
        process.join()
        if process.exitcode != 0:
            raise RuntimeError(
                f"Workflow inspection worker exited with code {process.exitcode}: {module_name}"
            )
        if not isinstance(outcome, (_EntrySuccess, _EntryFailure)):
            raise RuntimeError(
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
    before = _registration_count()
    try:
        module = importlib.import_module(module_name)
    except (Exception, SystemExit) as error:
        return _EntryFailure(_pack_diagnostic(error, root))
    try:
        module_file = vars(module).get("__file__")
        actual_source = (
            Path(module_file).resolve(strict=True) if type(module_file) is str else None
        )
    except (OSError, RuntimeError) as error:
        return _EntryFailure(_pack_diagnostic(error, root))
    if actual_source != source:
        error = ValueError(
            f"Workflow entry {source.relative_to(root).as_posix()} "
            "loaded from an unexpected module path"
        )
        return _EntryFailure(_pack_diagnostic(error, root))
    registrations = _registrations_since(before)
    registration = registrations[0] if len(registrations) == 1 else None
    registration_module = (
        object.__getattribute__(registration, "__module__")
        if inspect.isfunction(registration)
        else None
    )
    if type(registration_module) is not str or registration_module != module_name:
        relative = source.relative_to(root).as_posix()
        error = ValueError(
            f"Workflow entry {relative} must register exactly one Workflow "
            "defined by that module"
        )
        return _EntryFailure(_pack_diagnostic(error, root))
    try:
        compiled = compile_declared_workflow(registration)
    except ValueError as error:
        return _EntryFailure(_pack_diagnostic(error, root))
    return _EntrySuccess(compiled.interface)


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
