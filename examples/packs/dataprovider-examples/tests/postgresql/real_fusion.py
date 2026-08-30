from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from kat import dataprovider as dp
from kat.pack.datasources.postgresql import PostgreSQLProvider


_CLOCK_DOMAIN = "fixture.observation_clock"


EXPECTED_SCHEMA = pa.schema(
    [
        ("thread_id", pa.int64()),
        ("process_id", pa.int64()),
        ("process_name", pa.string()),
        ("clock_domain", pa.string()),
        ("clock_value", pa.int64()),
        ("cpu", pa.int64()),
        ("cpu_usage", pa.float64()),
    ]
)


def _placement(root: Path) -> Path:
    placement_root = root / "placement"
    placement_root.mkdir()
    pq.write_table(
        pa.table(
            {
                "thread_id": pa.array([101, 102, 103], type=pa.int64()),
                "cpu": pa.array([0, 1, 2], type=pa.int64()),
            }
        ),
        placement_root / "thread_placement.parquet",
    )
    return placement_root


def _run(
    kat_run,
    config,
    placement_root: Path,
    start_clock_value: int,
    end_clock_value: int,
):
    return kat_run(
        workflow="fuse-observations",
        arguments=[
            "--service",
            config.readonly_profile,
            "--telemetry-database",
            config.telemetry_database,
            "--control-database",
            config.control_database,
            "--placement-root",
            str(placement_root),
            "--clock-domain",
            _CLOCK_DOMAIN,
            "--start-clock-value",
            str(start_clock_value),
            "--end-clock-value",
            str(end_clock_value),
        ],
    )["main"]


def _run_single_source(
    kat_run,
    config,
    start_clock_value: int,
    end_clock_value: int,
):
    return kat_run(
        workflow="query-observations",
        arguments=[
            "--service",
            config.readonly_profile,
            "--database",
            config.telemetry_database,
            "--clock-domain",
            _CLOCK_DOMAIN,
            "--start-clock-value",
            str(start_clock_value),
            "--end-clock-value",
            str(end_clock_value),
        ],
    )["main"]


def _postgresql_table(
    provider: PostgreSQLProvider,
    database: str,
    sql: str,
) -> dp.Table:
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


def test_local_fixture_has_one_row_per_thread(tmp_path):
    table = pq.read_table(_placement(tmp_path) / "thread_placement.parquet")

    assert table.column("thread_id").to_pylist() == [101, 102, 103]


def test_single_source_workflow_returns_the_provider_table_directly(
    kat_run,
    postgresql_config,
):
    result = _run_single_source(kat_run, postgresql_config, 100, 220)

    assert result.schema == pa.schema(
        [
            ("thread_id", pa.int64()),
            ("clock_domain", pa.string()),
            ("clock_value", pa.int64()),
            ("cpu_usage", pa.float64()),
        ]
    )
    assert result.to_pylist() == [
        {
            "thread_id": 101,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 100,
            "cpu_usage": 0.25,
        },
        {
            "thread_id": 101,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 140,
            "cpu_usage": 0.3,
        },
        {
            "thread_id": 102,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 150,
            "cpu_usage": 0.5,
        },
        {
            "thread_id": 103,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 170,
            "cpu_usage": 0.6,
        },
        {
            "thread_id": 999,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 180,
            "cpu_usage": 0.7,
        },
        {
            "thread_id": 102,
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 200,
            "cpu_usage": 0.75,
        },
    ]


def test_workflow_fuses_two_databases_and_local_parquet(
    kat_run,
    postgresql_config,
    tmp_path,
):
    result = _run(kat_run, postgresql_config, _placement(tmp_path), 100, 220)

    assert result.schema == EXPECTED_SCHEMA
    assert result.to_pylist() == [
        {
            "thread_id": 101,
            "process_id": 10,
            "process_name": "renderer",
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 100,
            "cpu": 0,
            "cpu_usage": 0.25,
        },
        {
            "thread_id": 101,
            "process_id": 10,
            "process_name": "renderer",
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 140,
            "cpu": 0,
            "cpu_usage": 0.3,
        },
        {
            "thread_id": 102,
            "process_id": 20,
            "process_name": "system-server",
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 150,
            "cpu": 1,
            "cpu_usage": 0.5,
        },
        {
            "thread_id": 102,
            "process_id": 20,
            "process_name": "system-server",
            "clock_domain": _CLOCK_DOMAIN,
            "clock_value": 200,
            "cpu": 1,
            "cpu_usage": 0.75,
        },
    ]


def test_workflow_preserves_schema_when_the_window_has_no_rows(
    kat_run,
    postgresql_config,
    tmp_path,
):
    result = _run(
        kat_run,
        postgresql_config,
        _placement(tmp_path),
        1_000,
        1_100,
    )

    assert result.schema == EXPECTED_SCHEMA
    assert result.num_rows == 0
