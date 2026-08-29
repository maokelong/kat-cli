from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
import re
import shutil
from typing import Self

from kat import datasource as ds


FTRACE_SCHEMA = ds.Schema(
    {
        "capture": {
            "tracer": str,
            "clock_domain": str,
            "ticks_per_second": int,
            "entries_in_buffer": int,
            "entries_written": int,
            "cpu_count": int,
        },
        "events": {
            "event_index": int,
            "clock_domain": str,
            "clock_value": int,
            "cpu": int,
            "comm": str,
            "pid": int,
            "tgid": int | None,
            "flags": str,
            "event": str,
            "details": str,
        },
    }
)

_BATCH_SIZE = 4096
_TRACER = re.compile(r"^#\s*tracer:\s*(\S.*?)\s*$")
_ENTRIES = re.compile(
    r"^#\s*entries-in-buffer/entries-written:\s*(\d+)/(\d+)\s+#P:(\d+)\s*$"
)
_EVENT = re.compile(
    r"^\s*(?P<comm>.+)-(?P<pid>\d+)\s+"
    r"(?:\(\s*(?P<tgid>\d+|-+)\s*\)\s+)?"
    r"\[(?P<cpu>\d+)\]\s+(?P<flags>\S+)\s+"
    r"(?P<timestamp>\d+(?:\.\d+)?):\s+"
    r"(?P<event>[^:\s]+):\s?(?P<details>.*)$"
)


class FtraceTextProvider:
    """Parse one tracefs text file into a reusable local Parquet catalog."""

    def __init__(
        self,
        *,
        source: Path,
        materialization_root: Path,
        clock_domain: str,
    ) -> None:
        if not isinstance(source, Path):
            raise TypeError("source must be a pathlib.Path")
        if not isinstance(materialization_root, Path):
            raise TypeError("materialization_root must be a pathlib.Path")
        if type(clock_domain) is not str:
            raise TypeError("clock_domain must be a string")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("clock_domain must be non-empty")
        self._source = source
        self._materialization_root = materialization_root
        self._clock_domain = clock_domain
        self._catalog: ds.Catalog | None = None

    def decode(self) -> Self:
        self._catalog = None
        self._materialization_root.mkdir(parents=True, exist_ok=True)
        catalog_root = self._materialization_root / "catalog"
        _remove_owned_catalog(catalog_root)

        tracer: str | None = None
        capture: tuple[int, int, int] | None = None
        event_index = 0
        event_columns = _empty_event_columns()
        with self._source.open("r", encoding="utf-8") as source, ds.write(
            FTRACE_SCHEMA,
            destination=catalog_root,
        ) as writer:
            for line_number, raw_line in enumerate(source, start=1):
                line = raw_line.rstrip("\r\n")
                if not line:
                    continue
                if line.startswith("#"):
                    tracer_match = _TRACER.fullmatch(line)
                    if tracer_match is not None:
                        tracer = tracer_match.group(1)
                    entries_match = _ENTRIES.fullmatch(line)
                    if entries_match is not None:
                        capture = tuple(int(value) for value in entries_match.groups())
                    continue

                match = _EVENT.fullmatch(line)
                if match is None:
                    raise ValueError(f"invalid tracefs event at line {line_number}")
                _append_event(
                    event_columns,
                    match,
                    event_index=event_index,
                    clock_domain=self._clock_domain,
                    line_number=line_number,
                )
                event_index += 1
                if len(event_columns["clock_value"]) == _BATCH_SIZE:
                    writer.write("events", **event_columns)
                    event_columns = _empty_event_columns()

            if tracer is None:
                raise ValueError("tracefs header is missing tracer")
            if capture is None:
                raise ValueError(
                    "tracefs header is missing entries-in-buffer/entries-written"
                )
            entries_in_buffer, entries_written, cpu_count = capture
            writer.write(
                "capture",
                tracer=[tracer],
                clock_domain=[self._clock_domain],
                ticks_per_second=[1_000_000_000],
                entries_in_buffer=[entries_in_buffer],
                entries_written=[entries_written],
                cpu_count=[cpu_count],
            )
            if event_columns["clock_value"]:
                writer.write("events", **event_columns)

        self._catalog = ds.open(FTRACE_SCHEMA, root=catalog_root)
        return self

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> ds.Table:
        catalog = self._catalog
        if catalog is None:
            raise RuntimeError("decode() must complete before query()")
        return catalog.query(sql, params=params)


def _remove_owned_catalog(catalog_root: Path) -> None:
    if catalog_root.is_symlink() or catalog_root.is_file():
        catalog_root.unlink()
    elif catalog_root.is_dir():
        shutil.rmtree(catalog_root)


def _empty_event_columns() -> dict[str, list[object | None]]:
    return {name: [] for name in FTRACE_SCHEMA["events"]}


def _append_event(
    columns: dict[str, list[object | None]],
    match: re.Match[str],
    *,
    event_index: int,
    clock_domain: str,
    line_number: int,
) -> None:
    tgid = match.group("tgid")
    columns["event_index"].append(event_index)
    columns["clock_domain"].append(clock_domain)
    columns["clock_value"].append(
        _clock_value(match.group("timestamp"), line_number=line_number)
    )
    columns["cpu"].append(int(match.group("cpu")))
    columns["comm"].append(match.group("comm").strip())
    columns["pid"].append(int(match.group("pid")))
    columns["tgid"].append(None if tgid is None or tgid.startswith("-") else int(tgid))
    columns["flags"].append(match.group("flags"))
    columns["event"].append(match.group("event"))
    columns["details"].append(match.group("details"))


def _clock_value(value: str, *, line_number: int) -> int:
    seconds, separator, fraction = value.partition(".")
    if separator and len(fraction) > 9:
        raise ValueError(
            f"tracefs clock value has more than 9 fractional digits at line {line_number}"
        )
    return int(seconds) * 1_000_000_000 + int(fraction.ljust(9, "0") or "0")
