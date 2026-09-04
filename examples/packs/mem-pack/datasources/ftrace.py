from __future__ import annotations

import unicodedata
from collections.abc import Mapping
from pathlib import Path

import pyarrow.parquet as pq
from kat_datasource import text_ftrace

import kat
from kat import dataprovider as dp


_WINDOWS_DEVICE_NAMES = frozenset(
    {"con", "prn", "aux", "nul"}
    | {f"com{index}" for index in range(1, 10)}
    | {f"lpt{index}" for index in range(1, 10)}
)
_WINDOWS_FORBIDDEN_CHARACTERS = frozenset('<>:"/\\|?*')
_RELATION_SCHEMAS = {
    text_ftrace.HEADER_RELATION: (
        ("tracer", "string", False),
        ("has_tgid_column", "bool", False),
    ),
    text_ftrace.OCCURRENCE_RELATION: (
        ("_kat_row_id", "uint64", False),
        ("source_event_sequence", "uint64", False),
    ),
    text_ftrace.EVENT_RELATION: (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("clock_domain", "string", False),
        ("clock_value", "uint64", False),
        ("cpu", "uint32", False),
        ("emitter_thread_name", "string", False),
        ("emitter_thread_id", "int32", False),
        ("emitter_process_id", "int32", True),
        ("context_flags", "string", False),
    ),
    text_ftrace.UNSUPPORTED_EVENT_RELATION: (("event_name", "string", False),),
    "text_ftrace_event_sched_switch": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("previous_thread_name", "string", False),
        ("previous_thread_id", "int32", False),
        ("previous_priority", "int32", False),
        ("previous_state", "string", False),
        ("next_thread_name", "string", False),
        ("next_thread_id", "int32", False),
        ("next_priority", "int32", False),
    ),
    "text_ftrace_event_sched_wakeup": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("thread_name", "string", False),
        ("thread_id", "int32", False),
        ("priority", "int32", False),
        ("target_cpu", "uint32", False),
    ),
    "text_ftrace_event_sched_wakeup_new": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("thread_name", "string", False),
        ("thread_id", "int32", False),
        ("priority", "int32", False),
        ("target_cpu", "uint32", False),
    ),
    "text_ftrace_event_tracing_mark_write": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("content", "string", False),
    ),
    "text_ftrace_event_sched_blocked_reason": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("pid", "int32", False),
        ("io_wait", "uint32", False),
        ("caller", "string", False),
    ),
    "text_ftrace_event_mm_filemap_add_to_page_cache": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("device_major", "uint32", False),
        ("device_minor", "uint32", False),
        ("inode", "uint64", False),
        ("page_frame_number", "uint64", False),
        ("offset_bytes", "uint64", False),
        ("order", "uint32", True),
        ("page_address", "string", True),
    ),
    "text_ftrace_event_mm_filemap_delete_from_page_cache": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("device_major", "uint32", False),
        ("device_minor", "uint32", False),
        ("inode", "uint64", False),
        ("page_frame_number", "uint64", False),
        ("offset_bytes", "uint64", False),
        ("order", "uint32", True),
        ("page_address", "string", True),
    ),
    "text_ftrace_event_block_rq_issue": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("device_major", "uint32", False),
        ("device_minor", "uint32", False),
        ("rwbs", "string", False),
        ("bytes", "uint32", False),
        ("command", "string", False),
        ("sector", "uint64", False),
        ("sector_count", "uint32", False),
        ("process_name", "string", False),
    ),
    "text_ftrace_event_block_rq_complete": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("device_major", "uint32", False),
        ("device_minor", "uint32", False),
        ("rwbs", "string", False),
        ("command", "string", False),
        ("sector", "uint64", False),
        ("sector_count", "uint32", False),
        ("error", "int32", False),
    ),
    "text_ftrace_event_binder_transaction": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("transaction_id", "int32", False),
        ("destination_node_id", "int32", False),
        ("destination_process_id", "int32", False),
        ("destination_thread_id", "int32", False),
        ("reply", "int32", False),
        ("flags", "uint32", False),
        ("code", "uint32", False),
    ),
    "text_ftrace_event_print": (
        ("_kat_row_id", "uint64", False),
        ("_kat_parent_row_id", "uint64", False),
        ("instruction_pointer", "string", False),
        ("content", "string", False),
    ),
}
_PAYLOAD_RELATIONS = frozenset(_RELATION_SCHEMAS).difference(
    {
        text_ftrace.HEADER_RELATION,
        text_ftrace.OCCURRENCE_RELATION,
        text_ftrace.EVENT_RELATION,
        text_ftrace.UNSUPPORTED_EVENT_RELATION,
    }
)


@kat.provider(
    name="ftrace-text",
    description="将 tracefs 文本解码为可重复查询的类型化关系。",
    guide="providers/ftrace.md",
)
class FtraceProvider:
    """查询一份文本 Ftrace 所提供的类型化关系。"""

    def __init__(
        self,
        *,
        source: Path,
        clock_domain: str,
        workspace_root: Path,
    ) -> None:
        for field, value in (
            ("source", source),
            ("workspace_root", workspace_root),
        ):
            if not isinstance(value, Path):
                raise TypeError(f"Ftrace Provider {field} must be a Path")
        if type(clock_domain) is not str:
            raise TypeError("Ftrace Provider clock_domain must be a string")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("Ftrace Provider clock_domain must be non-empty")
        if not workspace_root.is_dir():
            raise RuntimeError("Ftrace Provider workspace_root must be a directory")

        self._clock_domain = clock_domain
        self._query_provider: dp.DataFusionProvider
        self._decode_report = text_ftrace.DecodeReport(unsupported_event_names=())
        self._tables: tuple[str, ...] = ()

        self._catalog_root = (
            workspace_root.resolve(strict=True) / _source_stem(source)
        )
        if _path_exists(self._catalog_root):
            self._open_catalog()
            return
        if not source.is_file():
            raise RuntimeError("Ftrace Provider source must be an existing file")
        source = source.resolve(strict=True)
        try:
            self._decode(source)
        except text_ftrace.DecodeError as error:
            if _path_exists(self._catalog_root):
                self._open_catalog()
                return
            raise RuntimeError(f"Ftrace Provider decode failed: {error}") from error
        self._open_catalog()

    def _decode(self, source: Path) -> None:
        """把来源转换到按 Source stem 确定的 Parquet Catalog。"""
        text_ftrace.decode(source, self._catalog_root, self._clock_domain)
        if not self._catalog_root.is_dir() or self._catalog_root.is_symlink():
            raise RuntimeError(
                "Ftrace Provider did not produce a regular catalog directory"
            )

    def _open_catalog(
        self,
    ) -> None:
        catalog = dp.open(root=self._catalog_root)
        relations = set(catalog.tables)
        unknown_relations = relations.difference(_RELATION_SCHEMAS)
        if unknown_relations:
            names = ", ".join(sorted(unknown_relations))
            raise RuntimeError(
                f"Ftrace Provider output contains unknown relations: {names}"
            )
        if text_ftrace.HEADER_RELATION not in relations:
            raise RuntimeError(
                f"Ftrace Provider output is missing {text_ftrace.HEADER_RELATION}"
            )
        if (text_ftrace.OCCURRENCE_RELATION in relations) != (
            text_ftrace.EVENT_RELATION in relations
        ):
            raise RuntimeError(
                "Ftrace Provider output must contain both "
                f"{text_ftrace.OCCURRENCE_RELATION} and "
                f"{text_ftrace.EVENT_RELATION}"
            )
        if relations.intersection(_PAYLOAD_RELATIONS) and not {
            text_ftrace.OCCURRENCE_RELATION,
            text_ftrace.EVENT_RELATION,
        }.issubset(relations):
            raise RuntimeError(
                "Ftrace Provider payload relations require both "
                f"{text_ftrace.OCCURRENCE_RELATION} and "
                f"{text_ftrace.EVENT_RELATION}"
            )
        if (
            text_ftrace.OCCURRENCE_RELATION not in relations
            and text_ftrace.UNSUPPORTED_EVENT_RELATION not in relations
        ):
            raise RuntimeError(
                "Ftrace Provider output without event relations must contain "
                f"{text_ftrace.UNSUPPORTED_EVENT_RELATION}"
            )
        expected_version = text_ftrace.MATERIALIZATION_VERSION.encode("utf-8")
        for relation in sorted(relations):
            schema = pq.read_schema(self._catalog_root / f"{relation}.parquet")
            metadata = schema.metadata or {}
            if (
                metadata.get(text_ftrace.MATERIALIZATION_VERSION_METADATA_KEY)
                != expected_version
            ):
                raise RuntimeError(
                    f"Ftrace Provider output relation {relation!r} has "
                    "an incompatible materialization version"
                )
            actual_schema = tuple(
                (field.name, str(field.type), field.nullable) for field in schema
            )
            expected_schema = _RELATION_SCHEMAS[relation]
            if actual_schema != expected_schema:
                raise RuntimeError(
                    f"Ftrace Provider output relation {relation!r} has "
                    "incompatible columns or types"
                )
        query_provider = dp.DataFusionProvider(catalog=catalog)
        if text_ftrace.EVENT_RELATION in relations:
            domains = {
                row["clock_domain"]
                for row in query_provider.query(
                    f"SELECT DISTINCT clock_domain FROM {text_ftrace.EVENT_RELATION}"
                ).to_rows()
            }
            if domains != {self._clock_domain}:
                raise RuntimeError(
                    "Ftrace Provider cached clock_domain does not match the request"
                )
        unsupported_event_names: tuple[str, ...] = ()
        if text_ftrace.UNSUPPORTED_EVENT_RELATION in relations:
            unsupported_event_names = tuple(
                row["event_name"]
                for row in query_provider.query(
                    "SELECT event_name FROM "
                    f"{text_ftrace.UNSUPPORTED_EVENT_RELATION} "
                    "ORDER BY event_name"
                ).to_rows()
            )
            if unsupported_event_names != tuple(sorted(set(unsupported_event_names))):
                raise RuntimeError(
                    "Ftrace Provider unsupported event report is not sorted and unique"
                )
        decode_report = text_ftrace.DecodeReport(
            unsupported_event_names=unsupported_event_names
        )
        self._query_provider = query_provider
        self._decode_report = decode_report
        self._tables = tuple(sorted(relations))

    @property
    def decode_report(self) -> text_ftrace.DecodeReport:
        return self._decode_report

    @property
    def tables(self) -> tuple[str, ...]:
        return self._tables

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        return self._query_provider.query(sql, params=params)


def _source_stem(source: Path) -> str:
    stem = source.stem
    device_name = stem.split(".", 1)[0].casefold()
    if (
        not stem
        or stem in {".", ".."}
        or stem.endswith((".", " "))
        or device_name in _WINDOWS_DEVICE_NAMES
        or any(
            character in _WINDOWS_FORBIDDEN_CHARACTERS
            or unicodedata.category(character) == "Cc"
            for character in stem
        )
    ):
        raise ValueError(f"invalid Ftrace Provider source stem: {stem!r}")
    return stem


def _path_exists(path: Path) -> bool:
    return path.exists() or path.is_symlink()
