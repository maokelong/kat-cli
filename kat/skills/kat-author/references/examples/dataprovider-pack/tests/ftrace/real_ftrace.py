from __future__ import annotations

import os
from pathlib import Path

import pytest

from kat.pack.datasources.ftrace import FtraceTextProvider


def _required_real_ftrace() -> Path:
    value = os.environ.get("KAT_TEST_FTRACE_PATH")
    if not value:
        pytest.fail("KAT_TEST_FTRACE_PATH must identify the real Ftrace fixture")
    path = Path(value)
    if not path.is_file():
        pytest.fail("KAT_TEST_FTRACE_PATH must identify a readable file")
    return path


def test_real_ftrace_header_events_cpus_first_event_and_workflow_output(
    kat_run,
    tmp_path,
):
    source = _required_real_ftrace()
    clock_domain = "real_fixture_clock"
    provider = FtraceTextProvider(
        source=source,
        catalog_root=tmp_path / "catalog",
        clock_domain=clock_domain,
    ).decode()

    capture = provider.query("SELECT * FROM capture")
    summary = provider.query(
        """
        SELECT COUNT(*) AS event_count, COUNT(DISTINCT cpu) AS cpu_count
        FROM events
        """
    )
    first = provider.query(
        """
        SELECT event_index, clock_domain, clock_value,
               cpu, comm, pid, tgid, flags, event, details
        FROM events
        ORDER BY event_index
        LIMIT 1
        """
    )

    assert capture.to_rows() == [
        {
            "tracer": "nop",
            "clock_domain": clock_domain,
            "ticks_per_second": 1_000_000_000,
            "entries_in_buffer": 44_344,
            "entries_written": 44_344,
            "cpu_count": 4,
        }
    ]
    assert summary.to_rows() == [{"event_count": 44_344, "cpu_count": 4}]
    assert first.to_rows() == [
        {
            "event_index": 0,
            "clock_domain": clock_domain,
            "clock_value": 2_488_887_356_926_000,
            "cpu": 1,
            "comm": "<idle>",
            "pid": 0,
            "tgid": None,
            "flags": "d....",
            "event": "cpu_idle",
            "details": "state=4294967295 cpu_id=1",
        }
    ]

    output = kat_run(
        workflow="summarize-ftrace-events",
        arguments=(
            "--trace-path",
            str(source),
            "--clock-domain",
            clock_domain,
        ),
    )["main"]

    assert sum(row["event_count"] for row in output.to_pylist()) == 44_344
