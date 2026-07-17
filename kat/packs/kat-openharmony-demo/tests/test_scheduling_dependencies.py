import pyarrow as pa
import pytest


EXPECTED_SCHEMA = pa.schema(
    [
        pa.field("clock_domain", pa.string(), nullable=False),
        pa.field("clock_value", pa.int64(), nullable=False),
        pa.field("duration_ns", pa.int64(), nullable=False),
        pa.field("frame_thread_id", pa.int64(), nullable=False),
        pa.field("frame_thread_name", pa.string(), nullable=False),
        pa.field("frame_thread_state", pa.string(), nullable=False),
        pa.field("frame_io_wait", pa.int64()),
        pa.field("frame_blocked_function", pa.string()),
        pa.field("blocker_thread_id", pa.int64(), nullable=False),
        pa.field("blocker_thread_name", pa.string(), nullable=False),
        pa.field("blocker_process_id", pa.int64(), nullable=False),
        pa.field("blocker_process_name", pa.string(), nullable=False),
        pa.field("blocker_thread_state", pa.string(), nullable=False),
        pa.field("blocker_cpu", pa.int64()),
        pa.field("blocker_io_wait", pa.int64()),
        pa.field("blocker_blocked_function", pa.string()),
    ]
)


def run(kat_run, dataset, process_name):
    return kat_run(
        workflow="first-frame-scheduling-dependencies",
        dataset=dataset,
        arguments=["--process-name", process_name],
    )["scheduling_dependencies"]


def test_attributes_the_earliest_frame_without_main_thread_preference(kat_run):
    output = run(kat_run, "dependencies", ".demo")

    assert output.schema.equals(EXPECTED_SCHEMA, check_metadata=False)
    assert output.to_pylist() == [
        {
            "clock_domain": "boottime",
            "clock_value": 100,
            "duration_ns": 20,
            "frame_thread_id": 10,
            "frame_thread_name": "render",
            "frame_thread_state": "D",
            "frame_io_wait": 1,
            "frame_blocked_function": "futex_wait",
            "blocker_thread_id": 30,
            "blocker_thread_name": "helper",
            "blocker_process_id": 3000,
            "blocker_process_name": "kernel",
            "blocker_thread_state": "Running",
            "blocker_cpu": 0,
            "blocker_io_wait": None,
            "blocker_blocked_function": None,
        },
        {
            "clock_domain": "boottime",
            "clock_value": 120,
            "duration_ns": 20,
            "frame_thread_id": 10,
            "frame_thread_name": "render",
            "frame_thread_state": "D",
            "frame_io_wait": 1,
            "frame_blocked_function": "futex_wait",
            "blocker_thread_id": 20,
            "blocker_thread_name": "worker",
            "blocker_process_id": 2000,
            "blocker_process_name": "worker_proc",
            "blocker_thread_state": "Running",
            "blocker_cpu": 1,
            "blocker_io_wait": None,
            "blocker_blocked_function": None,
        },
        {
            "clock_domain": "boottime",
            "clock_value": 140,
            "duration_ns": 20,
            "frame_thread_id": 10,
            "frame_thread_name": "render",
            "frame_thread_state": "R",
            "frame_io_wait": None,
            "frame_blocked_function": None,
            "blocker_thread_id": 10,
            "blocker_thread_name": "render",
            "blocker_process_id": 1000,
            "blocker_process_name": ".demo",
            "blocker_thread_state": "R",
            "blocker_cpu": None,
            "blocker_io_wait": None,
            "blocker_blocked_function": None,
        },
        {
            "clock_domain": "boottime",
            "clock_value": 160,
            "duration_ns": 40,
            "frame_thread_id": 10,
            "frame_thread_name": "render",
            "frame_thread_state": "Running",
            "frame_io_wait": None,
            "frame_blocked_function": None,
            "blocker_thread_id": 10,
            "blocker_thread_name": "render",
            "blocker_process_id": 1000,
            "blocker_process_name": ".demo",
            "blocker_thread_state": "Running",
            "blocker_cpu": 2,
            "blocker_io_wait": None,
            "blocker_blocked_function": None,
        },
    ]


def test_udk_irq_is_a_real_source_boundary_not_a_synthetic_node(kat_run):
    rows = run(kat_run, "udk_irq", ".irq-demo").to_pylist()

    assert len(rows) == 1
    assert rows[0]["duration_ns"] == 10
    assert rows[0]["blocker_thread_id"] == 40
    assert rows[0]["blocker_thread_name"] == "udk-irq"
    assert rows[0]["blocker_thread_state"] == "Running"


def test_io_worker_boundary_is_explicit_and_keeps_its_real_state(kat_run):
    rows = run(kat_run, "hmfs", ".hmfs-demo").to_pylist()

    assert len(rows) == 1
    assert rows[0]["blocker_thread_name"] == "hmfs"
    assert rows[0]["blocker_thread_state"] == "D"
    assert rows[0]["duration_ns"] == 10


def test_self_wake_uses_the_real_waiter_as_the_kernel_fallback(kat_run):
    rows = run(kat_run, "self_wake", ".self-wake").to_pylist()

    assert len(rows) == 1
    assert rows[0]["frame_thread_id"] == rows[0]["blocker_thread_id"] == 60
    assert rows[0]["blocker_thread_state"] == "S"


@pytest.mark.parametrize(
    ("dataset", "process_name"),
    [
        ("missing_process", ".absent"),
        ("missing_frame", ".no-frame"),
        ("incomplete", ".incomplete"),
        ("hmfs_txn", ".hmfs-txn-demo"),
        ("equal_window_chain", ".equal-window"),
    ],
)
def test_missing_target_or_incomplete_attribution_fails(kat_run, dataset, process_name):
    with pytest.raises(pytest.fail.Exception, match="KAT Workflow test execution failed"):
        run(kat_run, dataset, process_name)
