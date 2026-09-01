from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
from kat.pack.datasources import ftrace as provider_module

_FIXTURE = Path(__file__).parent / "fixtures" / "typed.ftrace"


def _write_summary_catalog(root: Path) -> None:
    root.mkdir()
    pq.write_table(
        pa.table(
            {
                "tracer": ["nop"],
                "entries_in_buffer": pa.array([5], type=pa.uint64()),
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
    pq.write_table(
        pa.table(
            {
                "_kat_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "_kat_parent_row_id": pa.array([0, 1, 2, 3], type=pa.uint64()),
                "clock_domain": ["fixture_clock"] * 4,
                "clock_value": pa.array([1, 2, 3, 4], type=pa.uint64()),
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


def test_workflow_publishes_an_eager_summary(kat_run, monkeypatch, tmp_path):
    def convert(_source, catalog, _clock_domain):
        _write_summary_catalog(catalog)

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
            "cpu_count": 4,
            "source_event_count": 5,
            "supported_event_count": 4,
        }
    ]
