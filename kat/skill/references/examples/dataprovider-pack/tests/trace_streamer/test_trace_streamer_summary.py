from pathlib import Path
import sys

from kat.pack.datasources import trace_streamer


def test_provider_returns_native_hook_summary(tmp_path: Path):
    source = tmp_path / "fixture.htrace"
    source.write_text(
        """
import sqlite3
import sys

connection = sqlite3.connect(sys.argv[2])
connection.execute(
    "CREATE TABLE native_hook(event_type TEXT NOT NULL, heap_size INTEGER NOT NULL)"
)
connection.executemany(
    "INSERT INTO native_hook VALUES (?, ?)",
    [
        ("AllocEvent", 7),
        ("AllocEvent", 11),
        ("FreeEvent", 5),
        ("MmapEvent", 13),
    ],
)
connection.commit()
connection.close()
""".strip(),
        encoding="utf-8",
    )
    provider = trace_streamer.TraceStreamerProvider(
        source=source,
        executable=Path(sys.executable),
        workspace=tmp_path / "decoded",
    ).decode()
    result = provider.query(
        trace_streamer.NATIVE_HOOK_SUMMARY_SQL,
        schema=trace_streamer.NATIVE_HOOK_SUMMARY_SCHEMA,
    )

    assert result.to_rows() == [
        {
            "event_type": "AllocEvent",
            "event_count": 2,
            "total_heap_size": 18,
        },
        {
            "event_type": "FreeEvent",
            "event_count": 1,
            "total_heap_size": 5,
        },
        {
            "event_type": "MmapEvent",
            "event_count": 1,
            "total_heap_size": 13,
        },
    ]
