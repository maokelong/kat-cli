from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
import re
import shutil
from typing import Protocol, Self

import kat
from kat import dataprovider as dp


FTRACE_SCHEMA = dp.Schema(
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
_INT64_MIN = -(2**63)
_INT64_MAX = 2**63 - 1


@kat.provider(
    name="ftrace-text",
    description="解析 tracefs Ftrace 文本并物化可重复查询的 Parquet 多表目录。",
    guide="providers/ftrace-text.md",
)
class FtraceTextProvider:
    """把一份 tracefs 文本解析到当前 Provider 独占的 Parquet Catalog。"""

    def __init__(
        self,
        *,
        source: Path,
        catalog_root: Path,
        clock_domain: str,
    ) -> None:
        if not isinstance(source, Path):
            raise TypeError("source must be a pathlib.Path")
        if not isinstance(catalog_root, Path):
            raise TypeError("catalog_root must be a pathlib.Path")
        if type(clock_domain) is not str:
            raise TypeError("clock_domain must be a string")
        clock_domain = clock_domain.strip()
        if not clock_domain:
            raise ValueError("clock_domain must be non-empty")
        self._source = source
        self._catalog_root = catalog_root
        self._clock_domain = clock_domain
        self._fusion: dp.DataFusionProvider | None = None

    def decode(self) -> Self:
        self._fusion = None
        owns_catalog = True
        try:
            _remove_owned_catalog(self._catalog_root)
            owns_catalog = False
            with dp.write(FTRACE_SCHEMA, destination=self._catalog_root) as sink:
                events = sink["events"]
                tracer: str | None = None
                capture: tuple[int, int, int] | None = None
                event_index = 0
                with self._source.open("r", encoding="utf-8") as source:
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
                                capture = tuple(
                                    int(value) for value in entries_match.groups()
                                )
                            continue

                        match = _EVENT.fullmatch(line)
                        if match is None:
                            raise ValueError(
                                f"invalid tracefs event at line {line_number}"
                            )
                        _append_event(
                            events,
                            match,
                            event_index=event_index,
                            clock_domain=self._clock_domain,
                            line_number=line_number,
                        )
                        event_index += 1

                if tracer is None:
                    raise ValueError("tracefs header is missing tracer")
                if capture is None:
                    raise ValueError(
                        "tracefs header is missing entries-in-buffer/entries-written"
                    )
                entries_in_buffer, entries_written, cpu_count = capture
                sink["capture"].append(
                    tracer=tracer,
                    clock_domain=self._clock_domain,
                    ticks_per_second=1_000_000_000,
                    entries_in_buffer=entries_in_buffer,
                    entries_written=entries_written,
                    cpu_count=cpu_count,
                )

            owns_catalog = True
            catalog = dp.open(
                tables={
                    "capture": self._catalog_root / "capture.parquet",
                    "events": self._catalog_root / "events.parquet",
                }
            )
            fusion = dp.DataFusionProvider(catalog=catalog)
        except Exception:
            if owns_catalog:
                _cleanup_owned_catalog(self._catalog_root)
            raise

        self._fusion = fusion
        return self

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> dp.Table:
        fusion = self._fusion
        if fusion is None:
            raise RuntimeError("decode() must complete before query()")
        return fusion.query(sql, params=params)


def _remove_owned_catalog(catalog_root: Path) -> None:
    if catalog_root.is_symlink() or catalog_root.is_file():
        catalog_root.unlink()
    elif catalog_root.is_dir():
        shutil.rmtree(catalog_root)


def _cleanup_owned_catalog(catalog_root: Path) -> None:
    try:
        _remove_owned_catalog(catalog_root)
    except OSError:
        pass


class _RowSink(Protocol):
    def append(self, **row_values: object | None) -> None: ...


def _append_event(
    sink: _RowSink,
    match: re.Match[str],
    *,
    event_index: int,
    clock_domain: str,
    line_number: int,
) -> None:
    try:
        tgid = match.group("tgid")
        row = {
            "event_index": _require_int64(event_index, field_name="event_index"),
            "clock_domain": clock_domain,
            "clock_value": _require_int64(
                _clock_value(
                    match.group("timestamp"),
                    line_number=line_number,
                ),
                field_name="clock_value",
            ),
            "cpu": _require_int64(int(match.group("cpu")), field_name="cpu"),
            "comm": match.group("comm").strip(),
            "pid": _require_int64(int(match.group("pid")), field_name="pid"),
            "tgid": None
            if tgid is None or tgid.startswith("-")
            else _require_int64(int(tgid), field_name="tgid"),
            "flags": match.group("flags"),
            "event": match.group("event"),
            "details": match.group("details"),
        }
    except (TypeError, ValueError, OverflowError) as error:
        raise ValueError(
            f"invalid tracefs event values at line {line_number}: {error}"
        ) from error
    sink.append(**row)


def _require_int64(value: int, *, field_name: str) -> int:
    if not _INT64_MIN <= value <= _INT64_MAX:
        raise ValueError(f"{field_name} is outside the signed 64-bit range")
    return value


def _clock_value(value: str, *, line_number: int) -> int:
    seconds, separator, fraction = value.partition(".")
    if separator and len(fraction) > 9:
        raise ValueError(
            f"tracefs clock value has more than 9 fractional digits at line {line_number}"
        )
    return int(seconds) * 1_000_000_000 + int(fraction.ljust(9, "0") or "0")
