from pathlib import Path
import sys


def test_workflow_returns_native_hook_summary(kat_run, tmp_path: Path):
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

    result = kat_run(
        workflow="summarize-native-hook",
        arguments=[
            "--source-path",
            str(source),
            "--trace-streamer-path",
            sys.executable,
        ],
    )["main"]

    assert result.to_pylist() == [
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
