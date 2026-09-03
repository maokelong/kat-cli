from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest
from kat.pack.datasources import ftrace as provider_module
from kat.pack.datasources.ftrace import FtraceProvider

from kat import dataprovider as dp

_FIXTURE = Path(__file__).parent / "fixtures" / "typed.ftrace"


def _write_catalog(
    root: Path,
    *,
    include_root: bool = True,
    clock_domain: str = "fixture_clock",
    unsupported_event_names: tuple[str, ...] = (),
) -> None:
    root.mkdir()
    pq.write_table(
        pa.table({"tracer": ["nop"], "has_tgid_column": [True]}),
        root / "text_ftrace_header.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "source_event_sequence": pa.array([0, 1, 3, 4], type=pa.uint64()),
            }
        ),
        root / "text_ftrace_event_occurrence.parquet",
    )
    if unsupported_event_names:
        pq.write_table(
            pa.table({"event_name": list(unsupported_event_names)}),
            root / "text_ftrace_unsupported_event.parquet",
        )
    if not include_root:
        return
    pq.write_table(
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "_kat_parent_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "clock_domain": [clock_domain] * 4,
                "clock_value": pa.array(
                    [1_000_000_000, 2_000_000_000, 3_000_000_000, 4_000_000_000],
                    type=pa.uint64(),
                ),
                "cpu": pa.array([2, 2, 2, 2], type=pa.uint32()),
                "emitter_thread_name": ["worker"] * 4,
                "emitter_thread_id": pa.array([7, 7, 7, 7], type=pa.int32()),
                "emitter_process_id": pa.array([7, 7, 7, 7], type=pa.int32()),
                "context_flags": ["d...."] * 4,
            }
        ),
        root / "text_ftrace_event.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "_kat_row_id": pa.array([0], type=pa.uint64()),
                "_kat_parent_row_id": pa.array([0], type=pa.uint64()),
                "previous_thread_name": ["old"],
                "previous_thread_id": pa.array([7], type=pa.int32()),
                "previous_priority": pa.array([120], type=pa.int32()),
                "previous_state": ["R+"],
                "next_thread_name": ["new"],
                "next_thread_id": pa.array([8], type=pa.int32()),
                "next_priority": pa.array([100], type=pa.int32()),
            }
        ),
        root / "text_ftrace_event_sched_switch.parquet",
    )


def _write_unknown_only_catalog(root: Path) -> None:
    root.mkdir()
    pq.write_table(
        pa.table({"tracer": ["nop"], "has_tgid_column": [True]}),
        root / "text_ftrace_header.parquet",
    )
    pq.write_table(
        pa.table({"event_name": ["a_event", "z_event"]}),
        root / "text_ftrace_unsupported_event.parquet",
    )


def _arguments(workspace_root: Path, **overrides) -> dict[str, object]:
    arguments = {
        "source": _FIXTURE,
        "clock_domain": "fixture_clock",
        "workspace_root": workspace_root,
    }
    arguments.update(overrides)
    return arguments


def test_construction_decodes_to_workspace_root_plus_source_name(
    monkeypatch,
    tmp_path,
):
    def convert(source, catalog_root, clock_domain):
        assert source == _FIXTURE.resolve()
        assert clock_domain == "fixture_clock"
        assert catalog_root == tmp_path / _FIXTURE.name
        _write_catalog(catalog_root)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)
    provider = FtraceProvider(**_arguments(tmp_path))

    topology = provider.query(
        """
        SELECT o.source_event_sequence, e.clock_domain, s.previous_state
        FROM text_ftrace_event_occurrence o
        JOIN text_ftrace_event e
          ON e._kat_parent_row_id = o._kat_row_id
        JOIN text_ftrace_event_sched_switch s
          ON s._kat_parent_row_id = e._kat_row_id
        """
    )

    assert isinstance(topology, dp.Table)
    assert topology.to_rows() == [
        {
            "source_event_sequence": 0,
            "clock_domain": "fixture_clock",
            "previous_state": "R+",
        }
    ]


def test_native_decoder_creates_and_reuses_the_file_name_catalog(tmp_path):
    catalog_root = tmp_path / _FIXTURE.name
    first = FtraceProvider(**_arguments(tmp_path))
    materialized_at = catalog_root.stat().st_mtime_ns

    second = FtraceProvider(**_arguments(tmp_path))

    assert first.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": 4}
    ]
    assert second.tables == first.tables
    assert catalog_root.stat().st_mtime_ns == materialized_at


def test_decode_failure_can_retry_when_no_catalog_was_written(monkeypatch, tmp_path):
    attempts = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            raise provider_module.text_ftrace.DecodeError("fixture failure")
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    with pytest.raises(RuntimeError, match="decode failed"):
        FtraceProvider(**_arguments(tmp_path))

    provider = FtraceProvider(**_arguments(tmp_path))
    assert provider.query(
        "SELECT COUNT(*) AS event_count FROM text_ftrace_event"
    ).to_rows() == [{"event_count": 4}]


def test_existing_parquet_is_not_redecoded_when_validation_fails(monkeypatch, tmp_path):
    conversions = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal conversions
        conversions += 1
        _write_catalog(catalog, include_root=False)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    with pytest.raises(RuntimeError, match="text_ftrace_event"):
        FtraceProvider(**_arguments(tmp_path))
    with pytest.raises(RuntimeError, match="text_ftrace_event"):
        FtraceProvider(**_arguments(tmp_path))

    assert conversions == 1
    assert (tmp_path / _FIXTURE.name).is_dir()


def test_query_provider_failure_keeps_the_materialized_catalog(monkeypatch, tmp_path):
    def convert(_source, catalog, _clock_domain):
        _write_catalog(catalog)

    def reject_catalog(*, catalog):
        assert catalog.tables
        raise RuntimeError("query provider failed")

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)
    monkeypatch.setattr(provider_module.dp, "DataFusionProvider", reject_catalog)

    with pytest.raises(RuntimeError, match="query provider failed"):
        FtraceProvider(**_arguments(tmp_path))

    assert (tmp_path / _FIXTURE.name).is_dir()


def test_same_file_name_reuses_the_materialized_catalog(monkeypatch, tmp_path):
    conversions = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal conversions
        conversions += 1
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    first = FtraceProvider(**_arguments(tmp_path))
    second = FtraceProvider(**_arguments(tmp_path))

    assert conversions == 1
    for provider in (first, second):
        assert provider.query(
            "SELECT COUNT(*) AS count FROM text_ftrace_event"
        ).to_rows() == [{"count": 4}]


def test_same_name_in_different_source_directories_reuses_catalog(
    monkeypatch, tmp_path
):
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    first_source = tmp_path / "first" / "trace.ftrace"
    second_source = tmp_path / "second" / "trace.ftrace"
    first_source.parent.mkdir()
    second_source.parent.mkdir()
    first_source.write_text("first", encoding="utf-8")
    second_source.write_text("second", encoding="utf-8")
    decoded_sources = []

    def convert(source, catalog, _clock_domain):
        decoded_sources.append(source)
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    FtraceProvider(**_arguments(workspace_root, source=first_source))
    FtraceProvider(**_arguments(workspace_root, source=second_source))

    assert decoded_sources == [first_source.resolve()]


def test_different_file_names_use_different_catalogs(monkeypatch, tmp_path):
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    first_source = tmp_path / "first.ftrace"
    second_source = tmp_path / "second.ftrace"
    first_source.write_text("first", encoding="utf-8")
    second_source.write_text("second", encoding="utf-8")
    catalogs = []

    def convert(_source, catalog, _clock_domain):
        catalogs.append(catalog)
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    FtraceProvider(**_arguments(workspace_root, source=first_source))
    FtraceProvider(**_arguments(workspace_root, source=second_source))

    assert catalogs == [
        workspace_root / first_source.name,
        workspace_root / second_source.name,
    ]


def test_cached_clock_domain_must_match_the_request(monkeypatch, tmp_path):
    conversions = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal conversions
        conversions += 1
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)
    FtraceProvider(**_arguments(tmp_path))

    with pytest.raises(RuntimeError, match="clock_domain"):
        FtraceProvider(**_arguments(tmp_path, clock_domain="another_clock"))

    assert conversions == 1


def test_empty_catalog_directory_is_decoded(monkeypatch, tmp_path):
    catalog_root = tmp_path / _FIXTURE.name
    catalog_root.mkdir()
    conversions = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal conversions
        conversions += 1
        _write_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    FtraceProvider(**_arguments(tmp_path))

    assert conversions == 1
    assert (catalog_root / "text_ftrace_header.parquet").is_file()


def test_nonempty_catalog_without_parquet_is_rejected(monkeypatch, tmp_path):
    catalog_root = tmp_path / _FIXTURE.name
    catalog_root.mkdir()
    marker = catalog_root / "keep.txt"
    marker.write_text("keep", encoding="utf-8")

    def convert(*_arguments):
        pytest.fail("nonempty catalog must not be overwritten")

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    with pytest.raises(RuntimeError, match="without Parquet must be empty"):
        FtraceProvider(**_arguments(tmp_path))

    assert marker.read_text(encoding="utf-8") == "keep"


def test_source_file_is_not_overwritten_when_it_matches_catalog_path(
    monkeypatch, tmp_path
):
    source = tmp_path / "trace.ftrace"
    source.write_text("trace", encoding="utf-8")

    def convert(*_arguments):
        pytest.fail("source path must not be used as a catalog")

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    with pytest.raises(RuntimeError, match="directory or absent"):
        FtraceProvider(**_arguments(tmp_path, source=source))

    assert source.read_text(encoding="utf-8") == "trace"


def test_unknown_only_catalog_is_queryable_and_preserves_the_decode_report(
    monkeypatch, tmp_path
):
    def convert(_source, catalog, _clock_domain):
        _write_unknown_only_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    first = FtraceProvider(**_arguments(tmp_path))
    second = FtraceProvider(**_arguments(tmp_path))

    assert first.tables == (
        "text_ftrace_header",
        "text_ftrace_unsupported_event",
    )
    assert first.decode_report.unsupported_event_names == ("a_event", "z_event")
    assert second.decode_report == first.decode_report
    assert first.query(
        "SELECT tracer, has_tgid_column FROM text_ftrace_header"
    ).to_rows() == [{"tracer": "nop", "has_tgid_column": True}]


@pytest.mark.parametrize("field", ("source", "workspace_root"))
def test_paths_require_pathlib_path(field, tmp_path):
    arguments = _arguments(tmp_path)
    arguments[field] = str(arguments[field])

    with pytest.raises(TypeError, match=rf"{field}.*Path"):
        FtraceProvider(**arguments)


def test_clock_domain_is_explicit_and_nonempty(tmp_path):
    with pytest.raises(TypeError, match="clock_domain.*string"):
        FtraceProvider(
            source=_FIXTURE,
            clock_domain=None,
            workspace_root=tmp_path,
        )
    with pytest.raises(ValueError, match="clock_domain.*non-empty"):
        FtraceProvider(
            source=_FIXTURE,
            clock_domain="   ",
            workspace_root=tmp_path,
        )


def test_removed_lifecycle_options_are_not_part_of_the_interface(tmp_path):
    with pytest.raises(TypeError, match="unexpected keyword argument"):
        FtraceProvider(**_arguments(tmp_path), redecode=True)
    with pytest.raises(TypeError, match="unexpected keyword argument"):
        FtraceProvider(**_arguments(tmp_path), auto_cleanup=True)
