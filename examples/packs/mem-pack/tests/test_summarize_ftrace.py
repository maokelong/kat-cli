from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
from kat.pack.datasources import ftrace as provider_module

_FIXTURE = Path(__file__).parent / "fixtures" / "typed.ftrace"
_VERSION_METADATA = {b"kat.materialization.version": b"text-ftrace-v1"}


def _write_relation(root: Path, name: str, table: pa.Table) -> None:
    nullable_fields = (
        frozenset({"emitter_process_id"})
        if name == "text_ftrace_event"
        else frozenset()
    )
    schema = pa.schema(
        [
            pa.field(field.name, field.type, field.name in nullable_fields)
            for field in table.schema
        ],
        metadata=_VERSION_METADATA,
    )
    pq.write_table(
        pa.Table.from_arrays(table.columns, schema=schema),
        root / f"{name}.parquet",
    )


def _write_summary_catalog(root: Path) -> None:
    root.mkdir()
    _write_relation(
        root,
        "text_ftrace_header",
        pa.table(
            {
                "tracer": ["nop"],
                "has_tgid_column": [True],
            }
        ),
    )
    _write_relation(
        root,
        "text_ftrace_event_occurrence",
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "source_event_sequence": pa.array([0, 1, 3, 4], type=pa.uint64()),
            }
        ),
    )
    _write_relation(
        root,
        "text_ftrace_event",
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "_kat_parent_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "clock_domain": ["fixture_clock"] * 4,
                "clock_value": pa.array([1, 2, 3, 4], type=pa.uint64()),
                "cpu": pa.array([2, 2, 2, 2], type=pa.uint32()),
                "emitter_thread_name": ["worker"] * 4,
                "emitter_thread_id": pa.array([7, 7, 7, 7], type=pa.int32()),
                "emitter_process_id": pa.array([7, 7, 7, 7], type=pa.int32()),
                "context_flags": ["d...."] * 4,
            }
        ),
    )


def _write_unknown_summary_catalog(root: Path) -> None:
    root.mkdir()
    _write_relation(
        root,
        "text_ftrace_header",
        pa.table(
            {
                "tracer": ["nop"],
                "has_tgid_column": [True],
            }
        ),
    )
    _write_relation(
        root,
        "text_ftrace_unsupported_event",
        pa.table({"event_name": ["custom_event"]}),
    )


def test_workflow_publishes_an_eager_summary(kat_run, monkeypatch):
    conversions = 0

    def convert(_source, catalog, _clock_domain):
        nonlocal conversions
        conversions += 1
        _write_summary_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    arguments = (
        "--trace-path",
        str(_FIXTURE),
        "--clock-domain",
        "fixture_clock",
    )
    first = kat_run(workflow="summarize-ftrace", arguments=arguments)
    second = kat_run(workflow="summarize-ftrace", arguments=arguments)

    expected = [
        {
            "tracer": "nop",
            "supported_event_count": 4,
            "observed_cpu_count": 1,
        }
    ]
    assert first["main"].to_pylist() == expected
    assert second["main"].to_pylist() == expected
    assert conversions == 1


def test_workflow_reports_zero_supported_events(kat_run, monkeypatch):
    def convert(_source, catalog, _clock_domain):
        _write_unknown_summary_catalog(catalog)

    monkeypatch.setattr(provider_module.text_ftrace, "decode", convert)

    result = kat_run(
        workflow="summarize-ftrace",
        arguments=(
            "--trace-path",
            str(_FIXTURE),
            "--clock-domain",
            "fixture_clock",
        ),
    )

    assert result["main"].to_pylist() == [
        {
            "tracer": "nop",
            "supported_event_count": 0,
            "observed_cpu_count": 0,
        }
    ]
