from pathlib import Path

import pyarrow as pa
import pytest

from kat.pack.decoders.smaps import (
    MAPPINGS_SCHEMA,
    SNAPSHOTS_SCHEMA,
    SmapsDecodeError,
    mappings_reader,
    snapshots_reader,
)


FIXTURES = Path(__file__).parent / "fixtures"
SNAPSHOT_A = FIXTURES / "snapshot-a.smaps"
SNAPSHOT_B = FIXTURES / "snapshot-b.smaps"

EXPECTED_SNAPSHOTS_SCHEMA = pa.schema(
    [
        pa.field("snapshot_id", pa.uint64(), nullable=False),
        pa.field("source_file", pa.string(), nullable=False),
    ]
)
EXPECTED_MAPPINGS_SCHEMA = pa.schema(
    [
        pa.field("snapshot_id", pa.uint64(), nullable=False),
        pa.field("start_address", pa.uint64(), nullable=False),
        pa.field("end_address", pa.uint64(), nullable=False),
        pa.field("permissions", pa.string(), nullable=False),
        pa.field("offset", pa.uint64(), nullable=False),
        pa.field("device", pa.string(), nullable=False),
        pa.field("inode", pa.uint64(), nullable=False),
        pa.field("pathname", pa.string(), nullable=False),
        pa.field("size_kib", pa.uint64(), nullable=False),
        pa.field("rss_kib", pa.uint64(), nullable=False),
        pa.field("pss_kib", pa.uint64(), nullable=False),
    ]
)


def test_source_table_schemas_are_exact_and_non_nullable():
    assert SNAPSHOTS_SCHEMA.equals(EXPECTED_SNAPSHOTS_SCHEMA, check_metadata=False)
    assert MAPPINGS_SCHEMA.equals(EXPECTED_MAPPINGS_SCHEMA, check_metadata=False)


def test_decodes_mapping_headers_and_required_metrics():
    rows = mappings_reader((SNAPSHOT_A,)).read_all().to_pylist()

    assert rows == [
        {
            "snapshot_id": 0,
            "start_address": 0x00400000,
            "end_address": 0x00452000,
            "permissions": "r-xp",
            "offset": 0,
            "device": "08:02",
            "inode": 131073,
            "pathname": "/usr/bin/demo",
            "size_kib": 328,
            "rss_kib": 200,
            "pss_kib": 120,
        },
        {
            "snapshot_id": 0,
            "start_address": 0x00652000,
            "end_address": 0x00653000,
            "permissions": "r--p",
            "offset": 0x00052000,
            "device": "08:02",
            "inode": 131073,
            "pathname": "/usr/bin/demo",
            "size_kib": 4,
            "rss_kib": 4,
            "pss_kib": 2,
        },
        {
            "snapshot_id": 0,
            "start_address": 0x7F000000,
            "end_address": 0x7F002000,
            "permissions": "rw-p",
            "offset": 0,
            "device": "00:00",
            "inode": 0,
            "pathname": "[heap]",
            "size_kib": 8,
            "rss_kib": 8,
            "pss_kib": 8,
        },
    ]


def test_empty_file_has_stable_zero_row_tables():
    path = FIXTURES / "empty.smaps"
    snapshots = snapshots_reader((path,)).read_all()
    mappings = mappings_reader((path,)).read_all()

    assert snapshots.schema.equals(EXPECTED_SNAPSHOTS_SCHEMA, check_metadata=False)
    assert snapshots.to_pylist() == [{"snapshot_id": 0, "source_file": str(path)}]
    assert mappings.schema.equals(EXPECTED_MAPPINGS_SCHEMA, check_metadata=False)
    assert mappings.num_rows == 0


@pytest.mark.parametrize(
    ("fixture", "message"),
    [
        ("corrupt-header.smaps", "expected a SMAPS mapping header"),
        ("corrupt-metric.smaps", "Rss must be an unsigned KiB metric"),
    ],
)
def test_corrupt_header_or_required_metric_fails_the_reader(fixture, message):
    with pytest.raises(SmapsDecodeError, match=message):
        mappings_reader((FIXTURES / fixture,)).read_all()


def test_repeated_files_preserve_input_order_and_are_not_deduplicated():
    files = (SNAPSHOT_A, SNAPSHOT_B, SNAPSHOT_A)

    assert [
        row["snapshot_id"] for row in snapshots_reader(files).read_all().to_pylist()
    ] == [0, 1, 2]
    assert [
        row["snapshot_id"] for row in mappings_reader(files).read_all().to_pylist()
    ] == [0, 0, 0, 1, 1, 2, 2, 2]


def test_mapping_reader_can_emit_multiple_record_batches():
    reader = mappings_reader((SNAPSHOT_A,), batch_size=2)
    batches = list(reader)

    assert [batch.num_rows for batch in batches] == [2, 1]
    assert all(batch.schema.equals(EXPECTED_MAPPINGS_SCHEMA) for batch in batches)
