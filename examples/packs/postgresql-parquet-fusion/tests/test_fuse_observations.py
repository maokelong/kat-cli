import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import pyarrow as pa
import pyarrow.parquet as pq

from kat.pack.helpers.datasources.postgresql import PostgreSQLExecutor


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
    timestamp = [100, 140, 190, 220, 180, 200, 240, 160, 180]
    assert len(set(zip(cpu, timestamp, strict=True))) == len(cpu)
    trace_root = root / "trace"
    trace_root.mkdir()
    pq.write_table(
        pa.table(
            {
                "cpu": pa.array(cpu, type=pa.int64()),
                "next_thread_id": pa.array(
                    [101, 102, 0, 0, 0, 102, 0, 103, 0],
                    type=pa.int64(),
                ),
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
            "--profile",
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


def _postgresql_table(config, database: str, sql: str, tmp_path: Path) -> pa.Table:
    executor = PostgreSQLExecutor(
        profile=config.readonly_profile,
        database=database,
    )
    scratch = tmp_path / "fixture-query"
    scratch.mkdir(exist_ok=True)
    try:
        with executor.execute(sql, None, scratch=scratch) as reader:
            return reader.read_all()
    finally:
        executor.close()


def _runtime_poison_probe(tmp_path: Path, config) -> tuple[bytes, Path]:
    pack = tmp_path / "postgresql-poison-probe"
    datasource_root = pack / "helpers" / "datasources"
    workflow_root = pack / "workflows"
    datasource_root.mkdir(parents=True)
    workflow_root.mkdir()
    shutil.copyfile(
        Path(__file__).parents[1]
        / "helpers"
        / "datasources"
        / "postgresql.py",
        datasource_root / "postgresql.py",
    )
    (workflow_root / "probe.py").write_text(
        '''import kat

from kat.pack.helpers.datasources import postgresql


@kat.workflow(
    name="probe",
    title="Probe PostgreSQL failure",
    required_tables=[],
    parameters={"profile": "service", "database": "database"},
)
def probe(ctx: kat.Context, profile: str, database: str):
    """Catch a real source failure; the poisoned Context must still reject output."""
    try:
        postgresql.provider(
            ctx,
            profile=profile,
            database=database,
        ).query("SELECT 1::BIGINT AS value", name="failed")
    except RuntimeError:
        pass
    return None
''',
        encoding="utf-8",
    )

    data_home = tmp_path / "runtime-data-home"
    candidate_id = "019f6e00-0000-7000-8000-000000000001"
    candidate = data_home / "runs" / candidate_id
    candidate.mkdir(parents=True)
    request = {
        "operation": "run_workflow",
        "pack_name": "postgresql-poison-probe",
        "pack_path": str(pack.resolve()),
        "workflow_name": "probe",
        "arguments": [
            "--profile",
            config.readonly_profile,
            "--database",
            config.telemetry_database,
        ],
        "candidate_id": candidate_id,
        "candidate_path": str(candidate.resolve()),
        "datasource_root": str(
            data_home / "datasources" / "postgresql-poison-probe"
        ),
    }
    request_path = tmp_path / "runtime-request.json"
    response_path = tmp_path / "runtime-response.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    invalid_password = "kat-invalid-password-sentinel"
    environment = {**os.environ, "PGPASSWORD": invalid_password}
    completed = subprocess.run(
        [
            sys.executable,
            "-B",
            "-X",
            "utf8",
            "-u",
            "-m",
            "_kat_runtime",
            "--request",
            str(request_path),
            "--response",
            str(response_path),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        env=environment,
        timeout=30,
    )
    assert response_path.is_file(), completed.stderr.decode(errors="replace")
    response_bytes = response_path.read_bytes()
    combined = completed.stdout + completed.stderr + response_bytes
    assert invalid_password.encode() not in combined
    assert config.secret.encode() not in combined
    assert config.readonly_profile.encode() not in combined
    assert config.telemetry_database.encode() not in combined
    response = json.loads(response_bytes)
    assert response["status"] == "failure"
    assert "result" not in response
    assert "PostgreSQL query failed" in json.dumps(response)
    return combined, candidate


def test_fixture_contains_every_boundary_and_join_exclusion_sentinel(
    postgresql_config,
    tmp_path,
):
    telemetry = _postgresql_table(
        postgresql_config,
        postgresql_config.telemetry_database,
        """
        SELECT thread_id, observed_at, cpu_usage::TEXT AS cpu_usage
        FROM observation
        ORDER BY observed_at, thread_id
        """,
        tmp_path,
    )
    control = _postgresql_table(
        postgresql_config,
        postgresql_config.control_database,
        """
        SELECT process_id, process_name
        FROM process_registry
        ORDER BY process_id
        """,
        tmp_path,
    )

    assert telemetry.to_pylist() == [
        {"thread_id": 101, "observed_at": 99, "cpu_usage": "0.1"},
        {"thread_id": 101, "observed_at": 100, "cpu_usage": "0.25"},
        {"thread_id": 101, "observed_at": 140, "cpu_usage": "0.3"},
        {"thread_id": 102, "observed_at": 150, "cpu_usage": "0.5"},
        {"thread_id": 103, "observed_at": 170, "cpu_usage": "0.6"},
        {"thread_id": 999, "observed_at": 180, "cpu_usage": "0.7"},
        {"thread_id": 102, "observed_at": 200, "cpu_usage": "0.75"},
        {"thread_id": 102, "observed_at": 220, "cpu_usage": "0.9"},
    ]
    assert control.to_pylist() == [
        {"process_id": 10, "process_name": "renderer"},
        {"process_id": 20, "process_name": "system-server"},
    ]


def test_real_authentication_failure_poisons_context_and_publishes_nothing(
    postgresql_config,
    tmp_path,
):
    _, candidate = _runtime_poison_probe(tmp_path, postgresql_config)

    assert not (candidate / "manifest.json").exists()
    outputs = candidate / "outputs"
    assert (list(outputs.iterdir()) if outputs.exists() else []) == []
    assert not (candidate / ".scratch").exists()


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
