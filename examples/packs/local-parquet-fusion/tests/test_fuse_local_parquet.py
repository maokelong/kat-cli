from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from kat.pack.datasources.parquet import LocalParquetProvider


def _catalog(root: Path) -> tuple[Path, Path, Path]:
    events = root / "events.parquet"
    labels = root / "labels"
    owners = root / "owners.parquet"
    labels.mkdir()
    pq.write_table(
        pa.table(
            {
                "event_id": pa.array([1, 2, 3], type=pa.int64()),
                "owner_id": pa.array([10, 20, 20], type=pa.int64()),
                "score": pa.array([5, 15, 25], type=pa.int64()),
            }
        ),
        events,
    )
    pq.write_table(
        pa.table(
            {
                "event_id": pa.array([1, 2], type=pa.int64()),
                "label": pa.array(["boot", "render"], type=pa.string()),
            }
        ),
        labels / "part-0.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "event_id": pa.array([3], type=pa.int64()),
                "label": pa.array(["commit"], type=pa.string()),
            }
        ),
        labels / "part-1.parquet",
    )
    pq.write_table(
        pa.table(
            {
                "owner_id": pa.array([10, 20], type=pa.int64()),
                "owner_name": pa.array(["kernel", "graphics"], type=pa.string()),
            }
        ),
        owners,
    )
    return events, labels, owners


def _events_provider(events: Path, labels: Path) -> LocalParquetProvider:
    return LocalParquetProvider(
        tables={"events": events, "labels": labels},
    )


def _run(kat_run, paths: tuple[Path, Path, Path], minimum_score: int):
    events, labels, owners = paths
    return kat_run(
        workflow="fuse-local-parquet",
        arguments=[
            "--events-path",
            str(events),
            "--labels-path",
            str(labels),
            "--owners-path",
            str(owners),
            "--minimum-score",
            str(minimum_score),
        ],
    )["main"]


def test_provider_query_binds_named_parameters(tmp_path):
    events, labels, _ = _catalog(tmp_path)

    result = _events_provider(events, labels).query(
        "SELECT event_id, score FROM events WHERE score >= $minimum_score",
        params={"minimum_score": 20},
    )

    assert result.to_rows() == [{"event_id": 3, "score": 25}]


def test_tables_not_present_in_the_mapping_are_invisible(tmp_path):
    events, _, _ = _catalog(tmp_path)
    pq.write_table(pa.table({"secret": ["not mapped"]}), tmp_path / "hidden.parquet")
    provider = LocalParquetProvider(
        tables={"events": events},
    )

    with pytest.raises(Exception, match="(?i)hidden"):
        provider.query("SELECT * FROM hidden")


def test_query_result_is_an_eager_reusable_table(tmp_path):
    events, labels, _ = _catalog(tmp_path)

    result = _events_provider(events, labels).query(
        "SELECT event_id FROM events ORDER BY event_id"
    )

    assert result.columns == ("event_id",)
    assert result["event_id"] == (1, 2, 3)
    assert result["event_id"] == (1, 2, 3)


def test_provider_joins_a_single_file_and_a_sharded_directory(tmp_path):
    events, labels, _ = _catalog(tmp_path)

    result = _events_provider(events, labels).query(
        """
        SELECT event.event_id, label.label
        FROM events AS event
        JOIN labels AS label USING (event_id)
        ORDER BY event.event_id
        """
    )

    assert result.to_rows() == [
        {"event_id": 1, "label": "boot"},
        {"event_id": 2, "label": "render"},
        {"event_id": 3, "label": "commit"},
    ]


def test_workflow_binds_source_query_parameters(kat_run, tmp_path):
    result = _run(kat_run, _catalog(tmp_path), minimum_score=20)

    assert result.to_pylist() == [
        {
            "event_id": 3,
            "label": "commit",
            "owner_name": "graphics",
            "score": 25,
        }
    ]


def test_workflow_fuses_memory_table_with_parquet_catalog(kat_run, tmp_path):
    result = _run(kat_run, _catalog(tmp_path), minimum_score=0)

    assert result.to_pylist() == [
        {
            "event_id": 1,
            "label": "boot",
            "owner_name": "kernel",
            "score": 5,
        },
        {
            "event_id": 2,
            "label": "render",
            "owner_name": "graphics",
            "score": 15,
        },
        {
            "event_id": 3,
            "label": "commit",
            "owner_name": "graphics",
            "score": 25,
        },
    ]
