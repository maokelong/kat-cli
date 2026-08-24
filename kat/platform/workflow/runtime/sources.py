from __future__ import annotations

from collections.abc import Iterator, Mapping, Sequence
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
import threading
from typing import cast

from datafusion import SessionConfig, SessionContext
from datafusion.catalog import Catalog, CatalogProvider, SchemaProvider, Table
import pyarrow.dataset as ds

from kat._reader import (
    _adapt_schema_provider,
    _enumerable_schema_provider,
    _normalize_schema_provider,
    _reader_source_operation,
)

from .pack import (
    ProductionPack,
    SOURCE_RESOLUTION_PROFILE,
)
from .request import (
    ExternalSourceRef,
    MaterializedSourceRef,
    PackSearchRef,
    ResolvedDatasetRef,
    ResolvedSourceRef,
)


_PRIVATE_DEFAULT_CATALOG = "__kat_workflow__"
_PRIVATE_DEFAULT_SCHEMA = "__kat_workflow__"


@dataclass(frozen=True)
class SourceArgumentOverride:
    arguments: tuple[str, ...]
    argument_base: Path

    @classmethod
    def create(
        cls,
        arguments: Sequence[str],
        *,
        argument_base: Path,
    ) -> SourceArgumentOverride:
        if (
            isinstance(arguments, (str, bytes))
            or not isinstance(arguments, Sequence)
            or any(type(argument) is not str for argument in arguments)
        ):
            raise TypeError("Source arguments must be a sequence of strings")
        if not isinstance(argument_base, Path) or not argument_base.is_absolute():
            raise ValueError("Source argument base must be an absolute Path")
        return cls(arguments=tuple(arguments), argument_base=argument_base)


@dataclass
class _ProviderResolution:
    lock: threading.Lock = field(default_factory=threading.Lock)
    provider: object | None = None
    error: BaseException | None = None
    complete: bool = False


class _FailedSourceSchema(SchemaProvider):
    def __init__(self, pack: str, source: str, error: BaseException) -> None:
        self._pack = pack
        self._source = source
        self._error = error

    @property
    def table_names(self) -> tuple[str, ...]:  # type: ignore[override]
        return ()

    def table_exist(self, name: str) -> bool:
        return True

    def table(self, name: str) -> Table | None:
        raise RuntimeError(
            f"Source {self._pack}.{self._source} could not be resolved"
        ) from self._error


class _LazyPackCatalog(CatalogProvider):
    def __init__(
        self,
        resolver: _SourceResolver,
        pack: str,
        sources: set[str],
        fallback: Catalog | None = None,
    ) -> None:
        self._resolver = resolver
        self._pack = pack
        self._sources = frozenset(sources)
        self._fallback = fallback

    def schema_names(self) -> set[str]:
        names = set(self._sources)
        if self._fallback is not None:
            names.update(self._fallback.schema_names())
        return names

    def schema(self, name: str) -> object | None:
        if name not in self._sources:
            if self._fallback is None or name not in self._fallback.schema_names():
                return None
            return _adapt_schema_provider(self._fallback.schema(name))
        try:
            return self._resolver.resolve(self._pack, name)
        except (Exception, SystemExit) as error:
            # DataFusion 54 会吞掉 Python CatalogProvider.schema 回调直接抛出的
            # 异常。失败代理把原始异常保留到 DataFusion 解析目标表时再抛出。
            return _FailedSourceSchema(self._pack, name, error)


class _MaterializedSchema(SchemaProvider):
    def __init__(
        self,
        source: MaterializedSourceRef,
    ) -> None:
        self._tables = {table.name: table.path for table in source.tables}
        self._resolved: dict[str, Table] = {}
        self._lock = threading.Lock()

    @property
    def table_names(self) -> tuple[str, ...]:  # type: ignore[override]
        return tuple(sorted(self._tables))

    def table_exist(self, name: str) -> bool:
        return name in self._tables

    def table(self, name: str) -> Table | None:
        path = self._tables.get(name)
        if path is None:
            return None
        with self._lock:
            table = self._resolved.get(name)
            if table is None:
                # SchemaProvider 回调在 DataFusion 的 Tokio runtime 中执行；
                # PyArrow Dataset Provider 可避免再启动一个嵌套 runtime。
                table = Table(_materialized_dataset(path))
                self._resolved[name] = table
            return table


def _materialized_dataset(path: Path) -> ds.Dataset:
    return ds.dataset(path, format="parquet")


class _SourceResolver:
    def __init__(
        self,
        session: SessionContext,
        current_pack: ProductionPack | None,
        dataset: ResolvedDatasetRef | None,
        overrides: Mapping[str, SourceArgumentOverride],
        pack_paths: Mapping[str, Path],
        pack_search: PackSearchRef | None,
    ) -> None:
        self._session = session
        self._current_pack = current_pack
        self._dataset = dataset
        self._overrides = dict(overrides)
        self._pack_paths = dict(pack_paths)
        self._pack_search = pack_search
        self._packs: dict[str, ProductionPack] = {}
        if current_pack is not None:
            self._packs[current_pack.name] = current_pack
        self._resolutions: dict[tuple[str, str], _ProviderResolution] = {}
        self._resolutions_lock = threading.Lock()

    def resolve(self, pack: str, source: str) -> object:
        identity = (pack, source)
        with self._resolutions_lock:
            resolution = self._resolutions.setdefault(identity, _ProviderResolution())
        with resolution.lock:
            if resolution.complete:
                if resolution.error is not None:
                    raise RuntimeError(
                        f"Source {pack}.{source} failed earlier in this operation"
                    ) from resolution.error
                assert resolution.provider is not None
                return resolution.provider
            try:
                resolution.provider = _normalize_schema_provider(
                    self._resolve_once(pack, source),
                    session=self._session,
                )
            except (Exception, SystemExit) as error:
                resolution.error = error
                resolution.complete = True
                raise
            resolution.complete = True
            return resolution.provider

    def _resolve_once(self, pack: str, source: str) -> object:
        current_name = None if self._current_pack is None else self._current_pack.name
        override = self._overrides.get(source) if pack == current_name else None
        if override is not None:
            return self._invoke_source(pack, source, override)

        binding = None if self._dataset is None else self._dataset.source(pack, source)
        if isinstance(binding, MaterializedSourceRef):
            return _MaterializedSchema(binding)
        if isinstance(binding, ExternalSourceRef):
            return self._invoke_source(
                pack,
                source,
                SourceArgumentOverride(
                    arguments=binding.arguments,
                    argument_base=binding.working_directory,
                ),
            )

        if pack == current_name and source in self._declared_sources(self._current_pack):
            raise ValueError(
                f"Source {pack}.{source} has no Binding in the selected Dataset; "
                "bind or materialize it first"
            )
        raise ValueError(f"Source {pack}.{source} is not available in this operation")

    def _invoke_source(
        self,
        pack_name: str,
        source: str,
        override: SourceArgumentOverride,
    ) -> object:
        pack = self._source_pack(pack_name)
        if source not in self._declared_sources(pack):
            raise ValueError(
                f"External Binding {pack_name}.{source} no longer has "
                "a matching Source Entry"
            )
        compiled = pack.load_source(source)
        effective = compiled.parse_arguments(
            override.arguments,
            argument_base=override.argument_base,
        )
        return compiled.function(**effective)

    def _source_pack(self, pack: str) -> ProductionPack:
        loaded = self._packs.get(pack)
        if loaded is not None:
            return loaded
        path = self._pack_paths.get(pack)
        if path is None and self._pack_search is not None:
            path = self._select_query_pack(pack)
        if path is None:
            raise ValueError(
                f"External Binding {pack} has no discovered PACK; add it with --pack-dir"
            )
        loaded = ProductionPack.open(
            pack,
            path,
            profile=SOURCE_RESOLUTION_PROFILE,
        )
        self._packs[pack] = loaded
        return loaded

    def _select_query_pack(self, pack: str) -> Path:
        assert self._pack_search is not None
        candidates = self._pack_search.candidates.get(pack, ())
        if len(candidates) > 1:
            raise ValueError(
                f"External Binding {pack} has ambiguous PACK discovery "
                f"({len(candidates)} candidates)"
            )
        if not candidates:
            if self._pack_search.issues:
                raise ValueError(
                    f"External Binding {pack} has no discovered PACK; "
                    f"PACK discovery reported: {self._pack_search.issues[0]}"
                )
            raise ValueError(
                f"External Binding {pack} has no discovered PACK; add it with --pack-dir"
            )
        candidate = candidates[0]
        try:
            resolved = candidate.resolve(strict=True)
        except (OSError, RuntimeError):
            raise ValueError(
                f"External Binding {pack} discovered PACK directory no longer exists"
            ) from None
        if resolved != candidate or not resolved.is_dir():
            raise ValueError(
                f"External Binding {pack} discovered PACK path is not its canonical directory"
            )
        return resolved

    @staticmethod
    def _declared_sources(pack: ProductionPack | None) -> set[str]:
        if pack is None:
            return set()
        return {
            cast(str, entry.interface["name"])
            for entry in pack.source_entries
        }


class SourceOperation:
    def __init__(
        self,
        session: SessionContext,
        resolver: _SourceResolver,
        identities: set[tuple[str, str]],
    ) -> None:
        self.session = session
        self._resolver = resolver
        self._identities = frozenset(identities)

    def schema(self, pack: str, source: str) -> SchemaProvider:
        if (pack, source) not in self._identities:
            raise ValueError(
                f"Source {pack}.{source} is not available in this operation"
            ) from None
        return _enumerable_schema_provider(
            self._resolver.resolve(pack, source),
            session=self.session,
        )


@contextmanager
def open_source_operation(
    *,
    current_pack: ProductionPack | None,
    dataset: ResolvedDatasetRef | None,
    overrides: Mapping[str, SourceArgumentOverride] | None = None,
    pack_paths: Mapping[str, Path] | None = None,
    pack_search: PackSearchRef | None = None,
    staging_parent: Path | None = None,
    enable_url_table: bool = False,
) -> Iterator[SourceOperation]:
    copied_overrides = {} if overrides is None else dict(overrides)
    copied_pack_paths = {} if pack_paths is None else dict(pack_paths)
    if current_pack is None and copied_overrides:
        raise ValueError("Source overrides require a current PACK")

    default_catalog = (
        _PRIVATE_DEFAULT_CATALOG if current_pack is None else current_pack.name
    )
    configuration = (
        SessionConfig()
        .with_default_catalog_and_schema(default_catalog, _PRIVATE_DEFAULT_SCHEMA)
        .with_information_schema(True)
    )
    session = SessionContext(configuration)
    if enable_url_table:
        session = session.enable_url_table()

    resolver = _SourceResolver(
        session,
        current_pack,
        dataset,
        copied_overrides,
        copied_pack_paths,
        pack_search,
    )
    identities: set[tuple[str, str]] = set()
    if dataset is not None:
        identities.update((source.pack, source.source) for source in dataset.sources)
    if current_pack is not None:
        identities.update(
            (current_pack.name, cast(str, entry.interface["name"]))
            for entry in current_pack.source_entries
        )
        unknown_overrides = set(copied_overrides) - {
            source for pack, source in identities if pack == current_pack.name
        }
        if unknown_overrides:
            names = ", ".join(sorted(unknown_overrides))
            raise ValueError(f"unknown current PACK Source override: {names}")

    by_catalog: dict[str, set[str]] = {}
    for pack, source in identities:
        by_catalog.setdefault(pack, set()).add(source)
    for pack, sources in sorted(by_catalog.items()):
        fallback = (
            session.catalog(pack)
            if current_pack is not None and pack == current_pack.name
            else None
        )
        session.register_catalog_provider(
            pack,
            _LazyPackCatalog(resolver, pack, sources, fallback),
        )

    with _reader_source_operation(session, staging_parent=staging_parent):
        yield SourceOperation(session, resolver, identities)
