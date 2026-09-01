from __future__ import annotations

from dataclasses import dataclass
import heapq
import importlib
import inspect
import keyword
import os
from pathlib import Path
import re
import stat
from types import ModuleType
from typing import TypedDict

from kat._provider import _ProviderDeclaration

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .knowledge import read_guide
from .pack import _mount_current_pack


_PROVIDER_NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")
_MISSING = object()


class ProviderSummary(TypedDict):
    name: str
    description: str


class ProviderDetail(TypedDict):
    name: str
    description: str
    module: str
    qualname: str
    guide: str


@dataclass(frozen=True)
class InspectProvidersRuntimeResult:
    providers: list[ProviderSummary]


@dataclass(frozen=True)
class InspectProviderDetailRuntimeResult:
    provider: ProviderDetail


type InspectProviderRuntimeResult = (
    InspectProvidersRuntimeResult | InspectProviderDetailRuntimeResult
)


class ProviderInspectionError(Exception):
    """The selected PACK does not expose a valid Provider knowledge surface."""

    def __init__(self, diagnostic: RuntimeDiagnostic) -> None:
        super().__init__(diagnostic["message"])
        self.diagnostic = diagnostic


@dataclass(frozen=True)
class _ProviderModule:
    source: Path
    segments: tuple[str, ...]
    initializer: bool

    @property
    def module_name(self) -> str:
        return ".".join(("kat", "pack", "datasources", *self.segments))


@dataclass(frozen=True)
class _InspectedProvider:
    name: str
    description: str
    module: str
    qualname: str
    guide: str


def inspect_provider(
    selected_pack_name: str,
    pack_path: Path,
    provider_name: str | None = None,
) -> InspectProviderRuntimeResult:
    """Inspect Provider declarations and their knowledge guides in one PACK."""
    try:
        if not selected_pack_name:
            raise ValueError("PACK name must be a non-empty string")
        modules = _provider_modules(pack_path)
        providers = _load_providers(pack_path, modules)
        providers.sort(key=lambda provider: provider.name)
        names: set[str] = set()
        for provider in providers:
            if provider.name in names:
                raise ValueError(f"duplicate Provider name: {provider.name}")
            names.add(provider.name)
        if provider_name is None:
            return InspectProvidersRuntimeResult(
                providers=[
                    {
                        "name": provider.name,
                        "description": provider.description,
                    }
                    for provider in providers
                ]
            )
        selected = next(
            (provider for provider in providers if provider.name == provider_name),
            None,
        )
        if selected is None:
            raise ValueError(
                f"Provider {provider_name!r} was not found in the selected PACK"
            )
        return InspectProviderDetailRuntimeResult(
            provider={
                "name": selected.name,
                "description": selected.description,
                "module": selected.module,
                "qualname": selected.qualname,
                "guide": selected.guide,
            }
        )
    except (Exception, SystemExit) as error:
        if isinstance(error, ProviderInspectionError):
            raise
        raise ProviderInspectionError(
            diagnostic_from_exception(
                error,
                pack_path,
                message="Provider inspection failed",
                help="Correct the PACK Provider declarations and guides, then retry inspection",
            )
        ) from error


def _load_providers(
    root: Path, modules: list[_ProviderModule]
) -> list[_InspectedProvider]:
    if not modules:
        return []
    _mount_current_pack(root)
    providers: list[_InspectedProvider] = []
    seen_classes: set[int] = set()
    for entry in modules:
        module = importlib.import_module(entry.module_name)
        _verify_module_source(root, module, entry)
        for value in vars(module).values():
            if not inspect.isclass(value) or id(value) in seen_classes:
                continue
            class_module = type.__getattribute__(value, "__module__")
            if type(class_module) is not str or class_module != entry.module_name:
                continue
            declaration = type.__getattribute__(value, "__dict__").get(
                "__kat_provider__"
            )
            if declaration is None:
                continue
            if type(declaration) is not _ProviderDeclaration:
                raise ValueError(
                    f"Provider class {class_module}.{type.__getattribute__(value, '__qualname__')} "
                    "has invalid @kat.provider(...) metadata"
                )
            seen_classes.add(id(value))
            qualname = type.__getattribute__(value, "__qualname__")
            if type(qualname) is not str or not qualname:
                raise ValueError(
                    f"Provider {declaration.name!r} has an invalid class qualname"
                )
            if not _qualname_resolves_to(module, qualname, value):
                raise ValueError(
                    f"Provider class {class_module}.{qualname} cannot be imported "
                    "by its module and qualname"
                )
            if (
                type(declaration.name) is not str
                or _PROVIDER_NAME.fullmatch(declaration.name) is None
            ):
                raise ValueError(f"invalid Provider name: {declaration.name!r}")
            if (
                type(declaration.description) is not str
                or not declaration.description.strip()
            ):
                raise ValueError(
                    f"Provider {declaration.name!r} description must not be empty"
                )
            if (
                type(declaration.guide) is not str
                or not declaration.guide.strip()
            ):
                raise ValueError(
                    f"Provider {declaration.name!r} guide must not be empty"
                )
            providers.append(
                _InspectedProvider(
                    name=declaration.name,
                    description=declaration.description,
                    module=class_module,
                    qualname=qualname,
                    guide=read_guide(
                        root,
                        declaration.guide,
                        declaration="Provider",
                        category="providers",
                    ),
                )
            )
    return providers


def _qualname_resolves_to(
    module: ModuleType, qualname: str, provider_class: type[object]
) -> bool:
    segments = qualname.split(".")
    if any(
        not segment.isidentifier() or keyword.iskeyword(segment)
        for segment in segments
    ):
        return False
    module_namespace = ModuleType.__getattribute__(module, "__dict__")
    current = module_namespace.get(segments[0], _MISSING)
    for segment in segments[1:]:
        if not inspect.isclass(current):
            return False
        current = type.__getattribute__(current, "__dict__").get(segment, _MISSING)
    return current is provider_class


def _verify_module_source(
    root: Path, module: ModuleType, entry: _ProviderModule
) -> None:
    module_file = vars(module).get("__file__")
    actual_source = (
        Path(module_file).resolve(strict=True) if type(module_file) is str else None
    )
    if actual_source != entry.source:
        relative = entry.source.relative_to(root).as_posix()
        raise ValueError(
            f"Provider module {relative} loaded from an unexpected module path"
        )


def _provider_modules(root: Path) -> list[_ProviderModule]:
    directory = root / "datasources"
    try:
        try:
            metadata = directory.lstat()
        except FileNotFoundError:
            return []
        if not stat.S_ISDIR(metadata.st_mode):
            return []
        pending: list[tuple[str, Path, bool, bool]] = []
        modules: list[_ProviderModule] = []
        _enqueue_children(directory, directory, pending)
        while pending:
            _, path, is_directory, is_file = heapq.heappop(pending)
            if is_directory:
                _enqueue_children(directory, path, pending)
                continue
            if not is_file or path.suffix != ".py":
                continue
            relative = path.relative_to(directory)
            initializer = path.name == "__init__.py"
            segments = (
                tuple(relative.parts[:-1])
                if initializer
                else (*relative.parts[:-1], path.stem)
            )
            for segment in segments:
                if not segment.isidentifier() or keyword.iskeyword(segment):
                    raise ValueError(
                        f"invalid Provider module segment {segment!r} in "
                        f"{(Path('datasources') / relative).as_posix()}"
                    )
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(root) or not resolved.is_file():
                raise ValueError(
                    "Provider module is not an ordinary PACK file: "
                    f"{(Path('datasources') / relative).as_posix()}"
                )
            modules.append(
                _ProviderModule(
                    source=resolved,
                    segments=segments,
                    initializer=initializer,
                )
            )
    except OSError as error:
        raise OSError(f"failed to scan PACK datasources directory {directory}") from error
    modules.sort(key=lambda entry: entry.source.relative_to(root).as_posix())
    _validate_module_conflicts(modules)
    return modules


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


def _validate_module_conflicts(modules: list[_ProviderModule]) -> None:
    by_segments: dict[tuple[str, ...], _ProviderModule] = {}
    for module in modules:
        existing = by_segments.get(module.segments)
        if existing is not None:
            raise ValueError(
                "Provider module/package conflict between "
                f"{existing.source} and {module.source}"
            )
        by_segments[module.segments] = module
    for module in modules:
        if module.initializer:
            continue
        for other in modules:
            if (
                len(other.segments) > len(module.segments)
                and other.segments[: len(module.segments)] == module.segments
            ):
                raise ValueError(
                    "Provider module/package conflict between "
                    f"{module.source} and {other.source}"
                )
