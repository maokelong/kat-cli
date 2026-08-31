from pathlib import Path
import sqlite3
from tempfile import TemporaryDirectory

import pyarrow as pa
import pytest

from kat.pack.workflows.thread_cpu_time import thread_cpu_time


EXPECTED_SCHEMA = pa.schema(
    [
        pa.field("thread_id", pa.int32(), nullable=False),
        pa.field("thread_name", pa.string(), nullable=False),
        pa.field("cpu", pa.uint32(), nullable=False),
        pa.field("observed_cpu_time_ns", pa.int64(), nullable=False),
    ]
)

def run_workflow(sched_slices, threads):
    with TemporaryDirectory() as temporary:
        database = (Path(temporary) / "trace.db").resolve()
        connection = sqlite3.connect(database)
        try:
            connection.execute(
                "CREATE TABLE sched_slice(itid INT, dur INT, cpu INT)"
            )
            connection.execute("CREATE TABLE thread(itid INT, tid INT, name TEXT)")
            connection.executemany(
                "INSERT INTO sched_slice VALUES (:itid, :dur, :cpu)",
                sched_slices,
            )
            connection.executemany(
                "INSERT INTO thread VALUES (:itid, :tid, :name)",
                threads,
            )
            connection.commit()
        finally:
            connection.close()
        outputs = thread_cpu_time(object(), str(database))
        return outputs["thread_cpu_time_by_cpu"].to_arrow()


def semantic_source():
    return (
        [
            {"itid": 1, "dur": 40, "cpu": 0},
            {"itid": 1, "dur": 30, "cpu": 0},
            {"itid": 2, "dur": 20, "cpu": 0},
            {"itid": 2, "dur": 30, "cpu": 1},
            {"itid": 5, "dur": 0, "cpu": 2},
            {"itid": 0, "dur": 999, "cpu": 0},
            {"itid": 3, "dur": 10, "cpu": 0},
            {"itid": 4, "dur": None, "cpu": 0},
            {"itid": 6, "dur": 10, "cpu": None},
            {"itid": 99, "dur": 10, "cpu": 0},
        ],
        [
            {"itid": 0, "tid": 0, "name": "idle"},
            {"itid": 1, "tid": 10, "name": "worker"},
            {"itid": 2, "tid": 20, "name": "mover"},
            {"itid": 3, "tid": 30, "name": None},
            {"itid": 4, "tid": 31, "name": "partial"},
            {"itid": 5, "tid": 40, "name": "instant"},
            {"itid": 6, "tid": 50, "name": "unknown-cpu"},
        ],
    )


def test_aggregates_complete_non_idle_source_slices():
    sched_slices, threads = semantic_source()

    assert run_workflow(sched_slices, threads).to_pylist() == [
        {
            "thread_id": 10,
            "thread_name": "worker",
            "cpu": 0,
            "observed_cpu_time_ns": 70,
        },
        {
            "thread_id": 20,
            "thread_name": "mover",
            "cpu": 1,
            "observed_cpu_time_ns": 30,
        },
        {
            "thread_id": 20,
            "thread_name": "mover",
            "cpu": 0,
            "observed_cpu_time_ns": 20,
        },
        {
            "thread_id": 40,
            "thread_name": "instant",
            "cpu": 2,
            "observed_cpu_time_ns": 0,
        },
    ]


def test_excludes_unexplainable_slices_and_keeps_cpu_distinct():
    sched_slices, threads = semantic_source()
    rows = run_workflow(sched_slices, threads).to_pylist()

    assert {row["thread_id"] for row in rows}.isdisjoint({0, 30, 31, 50})
    assert {
        (row["cpu"], row["observed_cpu_time_ns"])
        for row in rows
        if row["thread_id"] == 20
    } == {(0, 20), (1, 30)}


def test_zero_complete_non_idle_slices_has_stable_empty_schema():
    table = run_workflow([], [])

    assert table.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert table.num_rows == 0


def test_rejects_source_values_that_do_not_fit_output_types():
    with pytest.raises(Exception, match="(?i)(cast|int32)"):
        run_workflow(
            [{"itid": 1, "dur": 1, "cpu": 0}],
            [{"itid": 1, "tid": 2**31, "name": "overflow"}],
        )


def test_rejects_observed_cpu_time_total_that_overflows_int64():
    with pytest.raises(Exception, match="(?i)(overflow|int64)"):
        run_workflow(
            [
                {"itid": 1, "dur": 2**62, "cpu": 0},
                {"itid": 1, "dur": 2**62, "cpu": 0},
            ],
            [{"itid": 1, "tid": 1, "name": "overflow"}],
        )


def test_workflow_reads_an_explicit_sqlite_provider_fixture(kat_run, tmp_path: Path):
    sched_slices, threads = semantic_source()
    database = (tmp_path / "trace.db").resolve()
    connection = sqlite3.connect(database)
    try:
        connection.execute("CREATE TABLE sched_slice(itid INT, dur INT, cpu INT)")
        connection.execute("CREATE TABLE thread(itid INT, tid INT, name TEXT)")
        connection.executemany(
            "INSERT INTO sched_slice VALUES (:itid, :dur, :cpu)",
            sched_slices,
        )
        connection.executemany(
            "INSERT INTO thread VALUES (:itid, :tid, :name)",
            threads,
        )
        connection.commit()
    finally:
        connection.close()

    output = kat_run(
        workflow="thread-cpu-time",
        arguments=["--sqlite-path", str(database)],
    )["thread_cpu_time_by_cpu"]

    assert output.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert output.to_pylist() == [
        {
            "thread_id": 10,
            "thread_name": "worker",
            "cpu": 0,
            "observed_cpu_time_ns": 70,
        },
        {
            "thread_id": 20,
            "thread_name": "mover",
            "cpu": 1,
            "observed_cpu_time_ns": 30,
        },
        {
            "thread_id": 20,
            "thread_name": "mover",
            "cpu": 0,
            "observed_cpu_time_ns": 20,
        },
        {
            "thread_id": 40,
            "thread_name": "instant",
            "cpu": 2,
            "observed_cpu_time_ns": 0,
        },
    ]
