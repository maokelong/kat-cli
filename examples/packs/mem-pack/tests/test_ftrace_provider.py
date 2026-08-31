import gc
from pathlib import Path
import subprocess

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from kat import dataprovider as dp
from kat.pack.datasources import ftrace as provider_module
from kat.pack.datasources.ftrace import FtraceProvider


_FIXTURE = Path(__file__).parent / "fixtures" / "typed.ftrace"


def _write_catalog(
    root: Path,
    *,
    include_root: bool = True,
    clock_domain: str = "fixture_clock",
) -> None:
    root.mkdir()
    pq.write_table(
        pa.table(
            {
                "tracer": ["nop"],
                "entries_in_buffer": pa.array([5], type=pa.uint64()),
                "entries_written": pa.array([5], type=pa.uint64()),
                "cpu_count": pa.array([4], type=pa.uint32()),
                "has_tgid_column": [True],
            }
        ),
        root / "text_ftrace_header.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "source_event_sequence": pa.array(
                    [0, 1, 3, 4], type=pa.uint64()
                ),
            }
        ),
        root / "text_ftrace_event_occurrence.parquet",
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
                "emitter_process_id": pa.array(
                    [7, 7, 7, 7], type=pa.int32()
                ),
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


def _arguments(tmp_path: Path, monkeypatch, **overrides) -> dict[str, object]:
    executable = tmp_path / "ftrace2parquet"
    executable.write_bytes(b"fixture executable")
    monkeypatch.setenv("KAT_FTRACE2PARQUET_EXECUTABLE", str(executable))
    arguments = {
        "source": _FIXTURE,
        "clock_domain": "fixture_clock",
        "workspace_root": tmp_path,
    }
    arguments.update(overrides)
    return arguments


def _catalog_directories(workspace_root: Path) -> list[Path]:
    cache_root = workspace_root / ".ftrace2parquet-cache"
    if not cache_root.exists():
        return []
    return sorted(path for path in cache_root.iterdir() if path.is_dir())


def test_construction_invokes_the_converter_and_exposes_typed_relations(
    monkeypatch,
    tmp_path,
):
    def convert(arguments, **options):
        assert arguments[1:3] == ["--input", str(_FIXTURE.resolve())]
        assert arguments[3] == "--output"
        assert arguments[5:] == ["--clock-domain", "fixture_clock"]
        catalog_root = Path(arguments[4])
        assert catalog_root.parent == tmp_path / ".ftrace2parquet-cache"
        assert len(catalog_root.name) == 64
        assert all(character in "0123456789abcdef" for character in catalog_root.name)
        assert options == {
            "cwd": catalog_root.parent,
            "shell": False,
            "stdin": subprocess.DEVNULL,
            "stdout": subprocess.DEVNULL,
            "stderr": subprocess.DEVNULL,
            "check": False,
        }
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)
    provider = FtraceProvider(**_arguments(tmp_path, monkeypatch))

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


def test_failed_constructor_cleans_partial_output_and_a_new_provider_can_retry(
    monkeypatch,
    tmp_path,
):
    attempts = 0

    def convert(arguments, **_options):
        nonlocal attempts
        attempts += 1
        catalog = Path(arguments[4])
        if attempts == 1:
            catalog.mkdir()
            (catalog / "partial").write_text("partial", encoding="utf-8")
            return subprocess.CompletedProcess(arguments, 9)
        _write_catalog(catalog)
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    with pytest.raises(RuntimeError, match="decode failed"):
        FtraceProvider(**_arguments(tmp_path, monkeypatch))
    assert _catalog_directories(tmp_path) == []

    provider = FtraceProvider(**_arguments(tmp_path, monkeypatch))
    assert provider.query(
        "SELECT COUNT(*) AS event_count FROM text_ftrace_event"
    ).to_rows() == [{"event_count": 4}]


def test_missing_required_relation_fails_closed(monkeypatch, tmp_path):
    def convert(arguments, **_options):
        _write_catalog(Path(arguments[4]), include_root=False)
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    with pytest.raises(RuntimeError, match="text_ftrace_event"):
        FtraceProvider(**_arguments(tmp_path, monkeypatch))

    assert _catalog_directories(tmp_path) == []


def test_query_provider_failure_cleans_the_converted_catalog(monkeypatch, tmp_path):
    def convert(arguments, **_options):
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 0)

    def reject_catalog(*, catalog):
        assert catalog.tables
        raise RuntimeError("query provider failed")

    monkeypatch.setattr(provider_module.subprocess, "run", convert)
    monkeypatch.setattr(provider_module.dp, "DataFusionProvider", reject_catalog)

    with pytest.raises(RuntimeError, match="query provider failed"):
        FtraceProvider(**_arguments(tmp_path, monkeypatch))

    assert _catalog_directories(tmp_path) == []


def test_same_file_content_reuses_the_materialized_catalog(monkeypatch, tmp_path):
    conversions = 0

    def convert(arguments, **_options):
        nonlocal conversions
        conversions += 1
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    first = FtraceProvider(**_arguments(tmp_path, monkeypatch))
    monkeypatch.delenv("KAT_FTRACE2PARQUET_EXECUTABLE")
    second = FtraceProvider(
        source=_FIXTURE,
        clock_domain="fixture_clock",
        workspace_root=tmp_path,
    )

    assert conversions == 1
    assert first.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": 4}
    ]
    assert second.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": 4}
    ]


def test_different_file_content_uses_a_different_catalog(monkeypatch, tmp_path):
    first_source = tmp_path / "first.ftrace"
    second_source = tmp_path / "second.ftrace"
    fixture = _FIXTURE.read_bytes()
    first_source.write_bytes(fixture)
    second_source.write_bytes(fixture + b"\n")
    catalogs = []

    def convert(arguments, **_options):
        catalog = Path(arguments[4])
        catalogs.append(catalog)
        _write_catalog(catalog)
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    FtraceProvider(
        **_arguments(tmp_path, monkeypatch, source=first_source)
    )
    FtraceProvider(
        **_arguments(tmp_path, monkeypatch, source=second_source)
    )

    assert len(catalogs) == 2
    assert catalogs[0] != catalogs[1]


def test_corrupt_cached_catalog_is_rebuilt(monkeypatch, tmp_path):
    conversions = 0

    def convert(arguments, **_options):
        nonlocal conversions
        conversions += 1
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    FtraceProvider(**_arguments(tmp_path, monkeypatch))
    [catalog_root] = _catalog_directories(tmp_path)
    (catalog_root / "text_ftrace_event.parquet").unlink()

    provider = FtraceProvider(**_arguments(tmp_path, monkeypatch))

    assert conversions == 2
    assert provider.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": 4}
    ]


def test_concurrent_publisher_winner_is_reused(monkeypatch, tmp_path):
    def convert(arguments, **_options):
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 9)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)

    provider = FtraceProvider(**_arguments(tmp_path, monkeypatch))

    assert provider.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": 4}
    ]


def test_cached_clock_domain_must_match_the_request(monkeypatch, tmp_path):
    conversions = 0

    def convert(arguments, **_options):
        nonlocal conversions
        conversions += 1
        _write_catalog(Path(arguments[4]))
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)
    FtraceProvider(**_arguments(tmp_path, monkeypatch))

    with pytest.raises(RuntimeError, match="clock_domain"):
        FtraceProvider(
            **_arguments(tmp_path, monkeypatch, clock_domain="another_clock")
        )

    assert conversions == 1
    assert len(_catalog_directories(tmp_path)) == 1


def test_auto_cleanup_uses_and_releases_a_private_catalog(monkeypatch, tmp_path):
    converted_catalog = None

    def convert(arguments, **_options):
        nonlocal converted_catalog
        converted_catalog = Path(arguments[4])
        _write_catalog(converted_catalog)
        return subprocess.CompletedProcess(arguments, 0)

    monkeypatch.setattr(provider_module.subprocess, "run", convert)
    provider = FtraceProvider(
        **_arguments(tmp_path, monkeypatch, auto_cleanup=True)
    )

    assert converted_catalog is not None
    assert converted_catalog.is_dir()
    assert converted_catalog.parent.name.startswith("ftrace-")
    assert _catalog_directories(tmp_path) == []

    del provider
    gc.collect()

    assert not converted_catalog.exists()


@pytest.mark.parametrize("field", ("source", "workspace_root"))
def test_paths_require_pathlib_path(field, tmp_path):
    arguments = {
        "source": _FIXTURE,
        "clock_domain": "fixture_clock",
        "workspace_root": tmp_path,
    }
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


def test_auto_cleanup_must_be_a_bool(tmp_path):
    with pytest.raises(TypeError, match="auto_cleanup.*bool"):
        FtraceProvider(
            source=_FIXTURE,
            clock_domain="fixture_clock",
            workspace_root=tmp_path,
            auto_cleanup=1,
        )


def test_converter_location_is_an_internal_deployment_detail(monkeypatch, tmp_path):
    monkeypatch.delenv("KAT_FTRACE2PARQUET_EXECUTABLE", raising=False)

    with pytest.raises(RuntimeError, match="KAT_FTRACE2PARQUET_EXECUTABLE"):
        FtraceProvider(
            source=_FIXTURE,
            clock_domain="fixture_clock",
            workspace_root=tmp_path,
        )
