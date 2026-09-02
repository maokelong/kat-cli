from pathlib import Path

import pytest

from kat.pack.datasources import ftrace as ftrace_module
from kat.pack.datasources.ftrace import FtraceTextProvider


_FIXTURE = Path(__file__).parent / "fixtures" / "small.ftrace"


def test_decode_pairs_clock_values_with_the_explicit_domain(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=tmp_path / "catalog",
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
            catalog_root=tmp_path / "wrong-type",
            clock_domain=None,
        )
    with pytest.raises(ValueError, match="clock_domain.*non-empty"):
        FtraceTextProvider(
            source=_FIXTURE,
            catalog_root=tmp_path / "empty",
            clock_domain="   ",
        )


def test_decode_exposes_capture_and_precise_events_as_queryable_tables(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=tmp_path / "catalog",
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
        catalog_root=tmp_path / "catalog",
        clock_domain="fixture_clock",
    )

    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_query_forwards_named_parameters_to_the_catalog(tmp_path):
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=tmp_path / "catalog",
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
        catalog_root=tmp_path / "catalog",
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
        catalog_root=tmp_path / "catalog",
        clock_domain="fixture_clock",
    )

    with pytest.raises(ValueError, match=r"invalid tracefs event at line 3$"):
        provider.decode()
    assert not (tmp_path / "catalog").exists()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")

    source.write_text(_FIXTURE.read_text(encoding="utf-8"), encoding="utf-8")
    provider.decode()

    assert provider.query("SELECT COUNT(*) AS event_count FROM events").to_rows() == [
        {"event_count": 3}
    ]


def test_event_value_outside_table_range_reports_its_source_line(tmp_path):
    source = tmp_path / "out-of-range.ftrace"
    source.write_text(
        "# tracer: nop\n"
        "# entries-in-buffer/entries-written: 1/1   #P:1\n"
        "worker-12 [18446744073709551616] ..... 1.0: cpu_idle: state=0\n",
        encoding="utf-8",
    )

    with pytest.raises(
        ValueError,
        match=r"invalid tracefs event values at line 3:",
    ):
        FtraceTextProvider(
            source=source,
            catalog_root=tmp_path / "catalog",
            clock_domain="fixture_clock",
        ).decode()

    assert not (tmp_path / "catalog").exists()


def test_catalog_open_failure_removes_written_files_and_keeps_provider_unready(
    monkeypatch,
    tmp_path,
):
    catalog_root = tmp_path / "catalog"
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=catalog_root,
        clock_domain="fixture_clock",
    )

    def fail_open(**_kwargs):
        raise RuntimeError("open failed")

    monkeypatch.setattr("kat.pack.datasources.ftrace.dp.open", fail_open)

    with pytest.raises(RuntimeError, match="open failed"):
        provider.decode()

    assert not catalog_root.exists()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_old_target_removal_failure_retries_cleanup_and_keeps_provider_unready(
    monkeypatch,
    tmp_path,
):
    catalog_root = tmp_path / "catalog"
    catalog_root.mkdir()
    (catalog_root / "stale").write_text("partial", encoding="utf-8")
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=catalog_root,
        clock_domain="fixture_clock",
    )
    original_remove = ftrace_module._remove_owned_catalog
    attempts = 0

    def fail_first_remove(path):
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            raise PermissionError("remove failed")
        original_remove(path)

    monkeypatch.setattr(ftrace_module, "_remove_owned_catalog", fail_first_remove)

    with pytest.raises(PermissionError, match="remove failed"):
        provider.decode()

    assert attempts == 2
    assert not catalog_root.exists()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_write_failure_removes_partial_catalog_and_keeps_provider_unready(
    monkeypatch,
    tmp_path,
):
    catalog_root = tmp_path / "catalog"
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=catalog_root,
        clock_domain="fixture_clock",
    )

    def fail_write(schema, *, destination):
        assert schema is ftrace_module.FTRACE_SCHEMA
        destination.mkdir()
        (destination / "partial.parquet").write_text("partial", encoding="utf-8")
        raise RuntimeError("write failed")

    monkeypatch.setattr("kat.pack.datasources.ftrace.dp.write", fail_write)

    with pytest.raises(RuntimeError, match="write failed"):
        provider.decode()

    assert not catalog_root.exists()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_backend_failure_removes_written_catalog_and_keeps_provider_unready(
    monkeypatch,
    tmp_path,
):
    catalog_root = tmp_path / "catalog"
    provider = FtraceTextProvider(
        source=_FIXTURE,
        catalog_root=catalog_root,
        clock_domain="fixture_clock",
    )

    def fail_backend(*, catalog):
        assert catalog.tables == ("capture", "events")
        raise RuntimeError("backend failed")

    monkeypatch.setattr(
        "kat.pack.datasources.ftrace.dp.DataFusionProvider",
        fail_backend,
    )

    with pytest.raises(RuntimeError, match="backend failed"):
        provider.decode()

    assert not catalog_root.exists()
    with pytest.raises(RuntimeError, match="decode.*before query"):
        provider.query("SELECT * FROM events")


def test_decode_rebuilds_only_the_exclusive_catalog_target(tmp_path):
    source = tmp_path / "batched.ftrace"
    event = "worker-7 [001] ..... 9.000000001: cpu_idle: state=0 cpu_id=1\n"
    source.write_text(
        "# tracer: nop\n"
        "# entries-in-buffer/entries-written: 17/17   #P:2\n"
        + event * 17,
        encoding="utf-8",
    )
    catalog_root = tmp_path / "catalog"
    sibling = tmp_path / "owned-by-workflow.txt"
    sibling.write_text("keep", encoding="utf-8")

    provider = FtraceTextProvider(
        source=source,
        catalog_root=catalog_root,
        clock_domain="fixture_clock",
    ).decode()
    first = provider.query("SELECT COUNT(*) AS event_count FROM events")
    (catalog_root / "stale").write_text("partial", encoding="utf-8")
    provider.decode()
    second = provider.query("SELECT COUNT(*) AS event_count FROM events")

    assert first.to_rows() == [{"event_count": 17}]
    assert second.to_rows() == [{"event_count": 17}]
    assert sorted(path.name for path in catalog_root.iterdir()) == [
        "capture.parquet",
        "events.parquet",
    ]
    assert sibling.read_text(encoding="utf-8") == "keep"
