from pathlib import Path


_FIXTURE = Path(__file__).parent / "fixtures" / "small.ftrace"


def test_workflow_decodes_ftrace_and_returns_event_counts(kat_run):
    outputs = kat_run(
        workflow="summarize-ftrace-events",
        arguments=(
            "--trace-path",
            str(_FIXTURE),
            "--clock-domain",
            "fixture_clock",
        ),
    )

    assert outputs["main"].to_pylist() == [
        {"event": "cpu_idle", "event_count": 1},
        {"event": "sched_switch", "event_count": 1},
        {"event": "tracing_mark_write", "event_count": 1},
    ]
