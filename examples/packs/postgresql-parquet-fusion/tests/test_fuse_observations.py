from itertools import pairwise
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from kat import datasource as ds
from kat.pack.datasources.postgresql import PostgreSQLProvider


EXPECTED_SCHEMA = pa.schema(
    [
        ("thread_id", pa.int64()),
        ("process_id", pa.int64()),
        ("process_name", pa.string()),
        ("observed_at", pa.int64()),
        ("cpu", pa.int64()),
        ("run_start", pa.int64()),
        ("run_end", pa.int64()),
        ("cpu_usage", pa.float64()),
    ]
)


def _trace(root: Path) -> Path:
    cpu = [0, 0, 0, 0, 1, 1, 1, 2, 2]
    next_thread_id = [101, 102, 0, 0, 0, 102, 0, 103, 0]
    timestamp = [100, 140, 190, 220, 180, 200, 240, 160, 180]
    assert len(set(zip(cpu, timestamp, strict=True))) == len(cpu)
    events_by_cpu: dict[int, list[tuple[int, int]]] = {}
    for event_cpu, thread_id, event_time in zip(
        cpu,
        next_thread_id,
        timestamp,
        strict=True,
    ):
        events_by_cpu.setdefault(event_cpu, []).append((event_time, thread_id))
    intervals_by_thread: dict[int, list[tuple[int, int]]] = {}
    for events in events_by_cpu.values():
        for (start, thread_id), (end, _) in pairwise(sorted(events)):
            if thread_id != 0:
                intervals_by_thread.setdefault(thread_id, []).append((start, end))
    for intervals in intervals_by_thread.values():
        assert all(
            previous_end <= following_start
            for (_, previous_end), (following_start, _) in pairwise(
                sorted(intervals)
            )
        )
    trace_root = root / "trace"
    trace_root.mkdir()
    pq.write_table(
        pa.table(
            {
                "cpu": pa.array(cpu, type=pa.int64()),
                "next_thread_id": pa.array(next_thread_id, type=pa.int64()),
                "timestamp": pa.array(timestamp, type=pa.int64()),
            }
        ),
        trace_root / "sched_switch.parquet",
    )
    return trace_root


def _run(kat_run, config, trace_root: Path, start_ns: int, end_ns: int):
    return kat_run(
        workflow="fuse-observations",
        arguments=[
            "--service",
            config.readonly_profile,
            "--telemetry-database",
            config.telemetry_database,
            "--control-database",
            config.control_database,
            "--trace-root",
            str(trace_root),
            "--start-ns",
            str(start_ns),
            "--end-ns",
            str(end_ns),
        ],
    )["main"]


def _run_single_source(kat_run, config, start_ns: int, end_ns: int):
    return kat_run(
        workflow="query-observations",
        arguments=[
            "--service",
            config.readonly_profile,
            "--database",
            config.telemetry_database,
            "--start-ns",
            str(start_ns),
            "--end-ns",
            str(end_ns),
        ],
    )["main"]


def _postgresql_table(
    provider: PostgreSQLProvider,
    database: str,
    sql: str,
) -> ds.Table:
    return provider.query(sql, database=database)


def test_fixture_contains_every_boundary_and_join_exclusion_sentinel(
    postgresql_config,
):
    provider = PostgreSQLProvider(service=postgresql_config.readonly_profile)
    telemetry = _postgresql_table(
        provider,
        postgresql_config.telemetry_database,
        """
        SELECT thread_id, observed_at, cpu_usage::TEXT AS cpu_usage
        FROM observation
        ORDER BY observed_at, thread_id
        """,
    )
    control = _postgresql_table(
        provider,
        postgresql_config.control_database,
        """
        SELECT process_id, process_name
        FROM process_registry
        ORDER BY process_id
        """,
    )

    assert telemetry.to_rows() == [
        {"thread_id": 101, "observed_at": 99, "cpu_usage": "0.1"},
        {"thread_id": 101, "observed_at": 100, "cpu_usage": "0.25"},
        {"thread_id": 101, "observed_at": 140, "cpu_usage": "0.3"},
        {"thread_id": 102, "observed_at": 150, "cpu_usage": "0.5"},
        {"thread_id": 103, "observed_at": 170, "cpu_usage": "0.6"},
        {"thread_id": 999, "observed_at": 180, "cpu_usage": "0.7"},
        {"thread_id": 102, "observed_at": 200, "cpu_usage": "0.75"},
        {"thread_id": 102, "observed_at": 220, "cpu_usage": "0.9"},
    ]
    assert control.to_rows() == [
        {"process_id": 10, "process_name": "renderer"},
        {"process_id": 20, "process_name": "system-server"},
    ]


def test_trace_fixture_has_no_overlapping_non_idle_thread_intervals(tmp_path):
    assert (_trace(tmp_path) / "sched_switch.parquet").is_file()


def test_single_source_workflow_returns_the_provider_table_directly(
    kat_run,
    postgresql_config,
):
    result = _run_single_source(kat_run, postgresql_config, 100, 220)

    assert result.schema == pa.schema(
        [
            ("thread_id", pa.int64()),
            ("observed_at", pa.int64()),
            ("cpu_usage", pa.float64()),
        ]
    )
    assert result.to_pylist() == [
        {"thread_id": 101, "observed_at": 100, "cpu_usage": 0.25},
        {"thread_id": 101, "observed_at": 140, "cpu_usage": 0.3},
        {"thread_id": 102, "observed_at": 150, "cpu_usage": 0.5},
        {"thread_id": 103, "observed_at": 170, "cpu_usage": 0.6},
        {"thread_id": 999, "observed_at": 180, "cpu_usage": 0.7},
        {"thread_id": 102, "observed_at": 200, "cpu_usage": 0.75},
    ]


def test_workflow_fuses_two_databases_and_local_parquet(
    kat_run,
    postgresql_config,
    tmp_path,
):
    result = _run(kat_run, postgresql_config, _trace(tmp_path), 100, 220)

    assert result.schema == EXPECTED_SCHEMA
    assert result.to_pylist() == [
        {
            "thread_id": 101,
            "process_id": 10,
            "process_name": "renderer",
            "observed_at": 100,
            "cpu": 0,
            "run_start": 100,
            "run_end": 140,
            "cpu_usage": 0.25,
        },
        {
            "thread_id": 102,
            "process_id": 20,
            "process_name": "system-server",
            "observed_at": 150,
            "cpu": 0,
            "run_start": 140,
            "run_end": 190,
            "cpu_usage": 0.5,
        },
        {
            "thread_id": 102,
            "process_id": 20,
            "process_name": "system-server",
            "observed_at": 200,
            "cpu": 1,
            "run_start": 200,
            "run_end": 240,
            "cpu_usage": 0.75,
        },
    ]


def test_workflow_preserves_schema_when_the_window_has_no_rows(
    kat_run,
    postgresql_config,
    tmp_path,
):
    result = _run(kat_run, postgresql_config, _trace(tmp_path), 1_000, 1_100)

    assert result.schema == EXPECTED_SCHEMA
    assert result.num_rows == 0
