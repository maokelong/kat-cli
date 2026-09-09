import os
from pathlib import Path
import subprocess

from kat.dataprovider.trace_streamer import TraceStreamerProvider
from kat.pack.workflows import summarize_native_hook as summary


def _required_file(name: str) -> Path:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"{name} must identify the real local fixture")
    path = Path(value)
    if not path.is_file():
        raise RuntimeError(f"{name} must identify an existing file")
    return path


def test_real_trace_streamer_native_hook_summary(tmp_path: Path):
    executable = _required_file("KAT_TEST_TRACE_STREAMER_EXE")
    version = subprocess.run(
        [str(executable), "--version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    # Trace Streamer 4.3.7 会把版本写到 stderr，并沿用历史上的非成功退出状态。
    assert version.returncode == 1
    assert version.stdout == ""
    assert version.stderr.strip() == "version 4.3.7"

    provider = TraceStreamerProvider(
        source=_required_file("KAT_TEST_HTRACE_PATH"),
        executable=executable,
        workspace_root=tmp_path,
    )

    result = provider.query(
        summary.NATIVE_HOOK_SUMMARY_SQL,
        schema=summary.NATIVE_HOOK_SUMMARY_SCHEMA,
    )

    assert result.to_rows() == [
        {
            "event_type": "AllocEvent",
            "event_count": 114_976,
            "total_heap_size": 21_964_373,
        },
        {
            "event_type": "FreeEvent",
            "event_count": 110_359,
            "total_heap_size": 20_577_720,
        },
        {
            "event_type": "MmapEvent",
            "event_count": 64,
            "total_heap_size": 11_538_432,
        },
        {
            "event_type": "MunmapEvent",
            "event_count": 57,
            "total_heap_size": 3_014_656,
        },
    ]
