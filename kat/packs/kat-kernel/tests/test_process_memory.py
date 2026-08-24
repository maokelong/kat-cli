from pathlib import Path

import pyarrow as pa
from datafusion import SessionContext

from kat.pack.analysis.process_memory import summarize_process_memory
from kat.pack.decoders.smaps import MAPPINGS_SCHEMA
from kat.pack.workflows.process_memory_summary import process_memory_summary


EXPECTED_SCHEMA = pa.schema(
    [
        pa.field("snapshot_id", pa.uint64(), nullable=False),
        pa.field("pathname", pa.string(), nullable=False),
        pa.field("rss_kib", pa.uint64(), nullable=False),
        pa.field("pss_kib", pa.uint64(), nullable=False),
    ]
)


class InMemoryContext:
    def __init__(self, rows):
        self.session = SessionContext()
        table = pa.Table.from_pylist(rows, schema=MAPPINGS_SCHEMA)
        batches = table.to_batches() or [pa.RecordBatch.from_pylist([], schema=MAPPINGS_SCHEMA)]
        self.session.register_record_batches("mappings", [batches])

    def sql(self, query, **params):
        return self.session.sql(
            query.replace("raw_smaps.mappings", "mappings"),
            param_values=params,
        )


def mapping(snapshot_id, pathname, rss_kib, pss_kib, start_address):
    return {
        "snapshot_id": snapshot_id,
        "start_address": start_address,
        "end_address": start_address + 4096,
        "permissions": "rw-p",
        "offset": 0,
        "device": "00:00",
        "inode": 0,
        "pathname": pathname,
        "size_kib": 4,
        "rss_kib": rss_kib,
        "pss_kib": pss_kib,
    }


def run_analysis(rows):
    return summarize_process_memory(InMemoryContext(rows)).to_arrow_table()


def test_analysis_aggregates_rss_and_pss_by_snapshot_and_pathname():
    table = run_analysis(
        [
            mapping(0, "/usr/bin/demo", 200, 120, 0x1000),
            mapping(0, "/usr/bin/demo", 4, 2, 0x2000),
            mapping(0, "[heap]", 8, 8, 0x3000),
            mapping(1, "/usr/bin/demo", 3, 2, 0x4000),
            mapping(1, "", 1, 1, 0x5000),
        ]
    )

    assert table.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert table.to_pylist() == [
        {"snapshot_id": 0, "pathname": "/usr/bin/demo", "rss_kib": 204, "pss_kib": 122},
        {"snapshot_id": 0, "pathname": "[heap]", "rss_kib": 8, "pss_kib": 8},
        {"snapshot_id": 1, "pathname": "/usr/bin/demo", "rss_kib": 3, "pss_kib": 2},
        {"snapshot_id": 1, "pathname": "", "rss_kib": 1, "pss_kib": 1},
    ]


def test_analysis_empty_result_keeps_exact_schema():
    table = run_analysis([])

    assert table.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert table.num_rows == 0


def test_workflow_only_wraps_the_reusable_analysis():
    outputs = process_memory_summary(
        InMemoryContext([mapping(0, "[heap]", 8, 7, 0x1000)])
    )

    assert set(outputs) == {"process_memory_by_pathname"}
    assert outputs["process_memory_by_pathname"].to_arrow_table().to_pylist() == [
        {"snapshot_id": 0, "pathname": "[heap]", "rss_kib": 8, "pss_kib": 7}
    ]


def test_kat_run_uses_real_source_arguments(kat_run):
    outputs = kat_run(
        workflow="process-memory-summary",
        sources={
            "raw_smaps": [
                "--files",
                str(Path("tests/fixtures/snapshot-a.smaps")),
                "--files",
                str(Path("tests/fixtures/snapshot-b.smaps")),
            ]
        },
    )

    assert outputs["process_memory_by_pathname"].to_pylist() == [
        {"snapshot_id": 0, "pathname": "/usr/bin/demo", "rss_kib": 204, "pss_kib": 122},
        {"snapshot_id": 0, "pathname": "[heap]", "rss_kib": 8, "pss_kib": 8},
        {"snapshot_id": 1, "pathname": "/usr/bin/demo", "rss_kib": 3, "pss_kib": 2},
        {"snapshot_id": 1, "pathname": "", "rss_kib": 1, "pss_kib": 1},
    ]
