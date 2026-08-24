import sys
from pathlib import Path
from types import SimpleNamespace

from kat.pack.sources.hitrace import hitrace


def test_source_constructs_the_private_native_provider(monkeypatch, tmp_path: Path):
    trace = tmp_path / "capture.htrace"
    sentinel = object()
    received = []

    def provider(path):
        received.append(path)
        return sentinel

    monkeypatch.setitem(
        sys.modules,
        "_kat_hitrace",
        SimpleNamespace(HitraceSchemaProvider=provider),
    )

    assert hitrace(trace) is sentinel
    assert received == [trace]
