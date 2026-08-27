from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from kat.pack.helpers.datasources.parquet import (
    LocalParquetExecutor,
    _record_batch_reader,
    provider,
)


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


def test_source_query_binds_named_parameters(kat_run, tmp_path):
    result = _run(kat_run, _catalog(tmp_path), minimum_score=20)

    assert result.to_pylist() == [
        {
            "event_id": 3,
            "label": "commit",
            "owner_name": "graphics",
            "score": 25,
        }
    ]


def test_tables_not_present_in_the_mapping_are_invisible(tmp_path):
    events, _, _ = _catalog(tmp_path)
    pq.write_table(pa.table({"secret": ["not mapped"]}), tmp_path / "hidden.parquet")
    executor = LocalParquetExecutor({"events": events})
    scratch = tmp_path / "scratch"
    scratch.mkdir()

    with pytest.raises(Exception, match="(?i)hidden"):
        with executor.execute(
            "SELECT * FROM hidden",
            None,
            scratch=scratch,
        ) as reader:
            reader.read_all()
    executor.close()


def test_catalog_acquisition_waits_for_context_entry(tmp_path):
    captured = []
    facade = object()

    class Context:
        def provider(self, executor):
            captured.append(executor)
            return facade

    assert provider(
        Context(),
        tables={"missing": tmp_path / "missing.parquet"},
    ) is facade
    executor = captured[0]
    scratch = tmp_path / "scratch"
    scratch.mkdir()

    manager = executor.execute("SELECT * FROM missing", None, scratch=scratch)
    assert executor._session is None
    with pytest.raises(Exception, match="(?i)(missing|parquet|path)"):
        with manager:
            pass
    executor.close()


def test_record_batch_adapter_is_exact_and_lazy():
    schema = pa.schema([("value", pa.int64())])
    pulled = []

    class Batch:
        def __init__(self, value):
            self.value = value

        def to_pyarrow(self):
            return pa.record_batch([[self.value]], schema=schema)

    class Frame:
        def schema(self):
            return schema

        def execute_stream(self):
            def batches():
                pulled.append(1)
                yield Batch(1)
                pulled.append(2)
                yield Batch(2)

            return batches()

    reader = _record_batch_reader(Frame())
    assert type(reader) is pa.RecordBatchReader
    assert pulled == []
    assert reader.read_next_batch().to_pydict() == {"value": [1]}
    assert pulled == [1]
    reader.close()


def test_executor_joins_a_single_file_and_a_sharded_directory(tmp_path):
    events, labels, _ = _catalog(tmp_path)
    executor = LocalParquetExecutor({"events": events, "labels": labels})
    scratch = tmp_path / "scratch"
    scratch.mkdir()

    with executor.execute(
        """
        SELECT event.event_id, label.label
        FROM events AS event
        JOIN labels AS label USING (event_id)
        ORDER BY event.event_id
        """,
        None,
        scratch=scratch,
    ) as reader:
        assert type(reader) is pa.RecordBatchReader
        result = reader.read_all()
    executor.close()

    assert result.to_pydict() == {
        "event_id": [1, 2, 3],
        "label": ["boot", "render", "commit"],
    }


def test_workflow_fuses_tables_from_separate_providers(kat_run, tmp_path):
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
