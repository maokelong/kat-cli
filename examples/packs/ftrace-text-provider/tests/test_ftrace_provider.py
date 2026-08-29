from pathlib import Path

import pytest

from kat.pack.datasources.ftrace import FtraceTextProvider


_FIXTURE = Path(__file__).parent / "fixtures" / "small.ftrace"


def test_decode_pairs_clock_values_with_the_explicit_domain(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    ).decode()

    events = provider.query(
        """
        SELECT event_index, clock_domain, clock_value
        FROM events
        ORDER BY event_index
        """
    )

    assert events.to_rows() == [
        {
            "event_index": 0,
            "clock_domain": "fixture_clock",
            "clock_value": 1_000_000_001,
        },
        {
            "event_index": 1,
            "clock_domain": "fixture_clock",
            "clock_value": 2_500_000_000,
        },
        {
            "event_index": 2,
            "clock_domain": "fixture_clock",
            "clock_value": 2_488_887_356_926_000,
        },
    ]


def test_clock_domain_must_be_a_nonempty_string(tmp_path):
    with pytest.raises(TypeError, match="clock_domain.*string"):
        FtraceTextProvider(
            source=_FIXTURE,
            materialization_root=tmp_path / "wrong-type",
            clock_domain=None,
        )
    with pytest.raises(ValueError, match="clock_domain.*non-empty"):
        FtraceTextProvider(
            source=_FIXTURE,
            materialization_root=tmp_path / "empty",
            clock_domain="   ",
        )


def test_decode_exposes_capture_and_precise_events_as_queryable_tables(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    ).decode()

    capture = provider.query("SELECT * FROM capture")
    events = provider.query(
        """
        SELECT event_index, clock_domain, clock_value,
               cpu, comm, pid, tgid, flags, event, details
        FROM events
        ORDER BY event_index
        """
    )

    assert capture.to_rows() == [
        {
            "tracer": "nop",
            "clock_domain": "fixture_clock",
            "ticks_per_second": 1_000_000_000,
            "entries_in_buffer": 3,
            "entries_written": 3,
            "cpu_count": 2,
        }
    ]
    assert events.to_rows() == [
        {
            "event_index": 0,
            "clock_domain": "fixture_clock",
            "clock_value": 1_000_000_001,
            "cpu": 1,
            "comm": "render-thread",
            "pid": 99,
            "tgid": 42,
            "flags": "d.h..",
            "event": "tracing_mark_write",
            "details": "B|99|frame",
        },
        {
            "event_index": 1,
            "clock_domain": "fixture_clock",
            "clock_value": 2_500_000_000,
            "cpu": 0,
            "comm": "<idle>",
            "pid": 0,
            "tgid": None,
            "flags": "d....",
            "event": "cpu_idle",
            "details": "state=0 cpu_id=0",
        },
        {
            "event_index": 2,
            "clock_domain": "fixture_clock",
            "clock_value": 2_488_887_356_926_000,
            "cpu": 1,
            "comm": "worker",
            "pid": 12,
            "tgid": 12,
            "flags": ".....",
            "event": "sched_switch",
            "details": "prev_comm=worker next_comm=idle",
        },
    ]


def test_query_before_decode_explains_that_decode_is_required(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    )

    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_query_forwards_named_parameters_to_the_catalog(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    ).decode()

    result = provider.query(
        "SELECT event FROM events WHERE event = $event",
        params={"event": "sched_switch"},
    )

    assert result.to_rows() == [{"event": "sched_switch"}]


def test_decode_maps_tracefs_missing_tgid_placeholder_to_null(tmp_path):
    source = tmp_path / "missing-tgid.ftrace"
    source.write_text(
        "# tracer: nop\n"
        "# entries-in-buffer/entries-written: 1/1   #P:1\n"
        "<idle>-0 (-----) [000] d.... 7.25: cpu_idle: state=0 cpu_id=0\n",
        encoding="utf-8",
    )

    provider = FtraceTextProvider(
        source=source,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    ).decode()

    assert provider.query("SELECT clock_value, tgid FROM events").to_rows() == [
        {"clock_value": 7_250_000_000, "tgid": None}
    ]


def test_bad_event_reports_its_line_and_the_same_provider_can_retry(tmp_path):
    source = tmp_path / "retry.ftrace"
    source.write_text(
        "# tracer: nop\n"
        "# entries-in-buffer/entries-written: 1/1   #P:1\n"
        "this is not a tracefs event\n",
        encoding="utf-8",
    )
    provider = FtraceTextProvider(
        source=source,
        materialization_root=tmp_path / "ftrace",
        clock_domain="fixture_clock",
    )

    with pytest.raises(ValueError, match=r"invalid tracefs event at line 3$"):
        provider.decode()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")

    source.write_text(_FIXTURE.read_text(encoding="utf-8"), encoding="utf-8")
    provider.decode()

    assert provider.query("SELECT COUNT(*) AS event_count FROM events").to_rows() == [
        {"event_count": 3}
    ]


def test_decode_writes_more_than_one_batch_without_touching_root_siblings(tmp_path):
    source = tmp_path / "batched.ftrace"
    event = "worker-7 [001] ..... 9.000000001: cpu_idle: state=0 cpu_id=1\n"
    source.write_text(
        "# tracer: nop\n"
        "# entries-in-buffer/entries-written: 4097/4097   #P:2\n"
        + event * 4097,
        encoding="utf-8",
    )
    materialization_root = tmp_path / "ftrace"
    materialization_root.mkdir()
    sibling = materialization_root / "owned-by-workflow.txt"
    sibling.write_text("keep", encoding="utf-8")

    provider = FtraceTextProvider(
        source=source,
        materialization_root=materialization_root,
        clock_domain="fixture_clock",
    ).decode()
    first = provider.query("SELECT COUNT(*) AS event_count FROM events")
    provider.decode()
    second = provider.query("SELECT COUNT(*) AS event_count FROM events")

    assert first.to_rows() == [{"event_count": 4097}]
    assert second.to_rows() == [{"event_count": 4097}]
    assert sibling.read_text(encoding="utf-8") == "keep"
