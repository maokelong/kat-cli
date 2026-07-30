import pyarrow as pa
import pytest


EXPECTED_SCHEMA = pa.schema(
    [
        pa.field("thread_id", pa.int32(), nullable=False),
        pa.field("thread_name", pa.string(), nullable=False),
        pa.field("cpu", pa.uint32(), nullable=False),
        pa.field("observed_cpu_time_ns", pa.int64(), nullable=False),
    ]
)


def test_aggregates_complete_non_idle_source_slices(kat_run):
    output = kat_run(workflow="thread-cpu-time", dataset="semantics")

    assert output["thread_cpu_time_by_cpu"].to_pylist() == [
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


def test_excludes_idle_unknown_duration_and_keeps_cpu_distinct(kat_run):
    rows = kat_run(workflow="thread-cpu-time", dataset="semantics")[
        "thread_cpu_time_by_cpu"
    ].to_pylist()

    assert all(row["thread_id"] != 0 for row in rows)
    assert {row["thread_id"] for row in rows}.isdisjoint({30, 31})
    mover_rows = {
        (row["cpu"], row["observed_cpu_time_ns"])
        for row in rows
        if row["thread_id"] == 20 and row["thread_name"] == "mover"
    }
    assert mover_rows == {
        (0, 20),
        (1, 30),
    }
    assert sum(
        row["observed_cpu_time_ns"]
        for row in rows
        if row["thread_id"] == 20
    ) == 50


def test_zero_complete_non_idle_slices_has_stable_empty_schema(kat_run):
    table = kat_run(workflow="thread-cpu-time", dataset="zero")[
        "thread_cpu_time_by_cpu"
    ]

    assert table.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert table.num_rows == 0


def test_rejects_source_values_that_do_not_fit_output_types(kat_run):
    with pytest.raises(pytest.fail.Exception):
        kat_run(workflow="thread-cpu-time", dataset="invalid")


def test_rejects_observed_cpu_time_total_that_overflows_int64(kat_run):
    with pytest.raises(pytest.fail.Exception):
        kat_run(workflow="thread-cpu-time", dataset="overflow")
