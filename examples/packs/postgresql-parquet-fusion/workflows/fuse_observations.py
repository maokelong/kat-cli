from pathlib import Path

import kat

from kat.pack.datasources.parquet import LocalParquetProvider
from kat.pack.datasources.postgresql import PostgreSQLProvider


@kat.workflow(
    name="fuse-observations",
    title="Fuse PostgreSQL observations with local scheduling",
    required_tables=[],
    parameters={
        "service": "libpq service name.",
        "telemetry_database": "Database containing observations.",
        "control_database": "Database containing process metadata.",
        "trace_root": "Directory containing sched_switch.parquet.",
        "start_ns": "Inclusive observation window start.",
        "end_ns": "Exclusive observation window end.",
    },
)
def fuse_observations(
    ctx: kat.Context,
    service: str,
    telemetry_database: str,
    control_database: str,
    trace_root: str,
    start_ns: int,
    end_ns: int,
):
    """顺序查询两个 Database 和本地 Parquet，再显式融合 eager Table。"""
    if start_ns >= end_ns:
        raise ValueError("start_ns must be less than end_ns")

    postgresql = PostgreSQLProvider(service=service)
    telemetry = postgresql.query(
        """
        SELECT
            o.thread_id,
            r.process_id,
            o.observed_at,
            AVG(o.cpu_usage)::DOUBLE PRECISION AS cpu_usage
        FROM observation AS o
        JOIN thread_registry AS r USING (thread_id)
        WHERE o.observed_at >= $1
          AND o.observed_at < $2
        GROUP BY o.thread_id, r.process_id, o.observed_at
        """,
        database=telemetry_database,
        params=(start_ns, end_ns),
    )

    processes = postgresql.query(
        """
        SELECT process_id, process_name
        FROM process_registry
        """,
        database=control_database,
    )

    switches = LocalParquetProvider(
        sched_switch=Path(trace_root) / "sched_switch.parquet",
    ).query(
        """
        WITH intervals AS (
            SELECT
                cpu,
                next_thread_id,
                timestamp AS run_start,
                LEAD(timestamp) OVER (
                    PARTITION BY cpu
                    ORDER BY timestamp
                ) AS run_end
            FROM sched_switch
        )
        SELECT cpu, next_thread_id, run_start, run_end
        FROM intervals
        WHERE run_start < $end_ns
          AND run_end > $start_ns
        """,
        params={"start_ns": start_ns, "end_ns": end_ns},
    )

    return ctx.sql(
        """
        SELECT
            t.thread_id,
            t.process_id,
            p.process_name,
            t.observed_at,
            s.cpu,
            s.run_start,
            s.run_end,
            t.cpu_usage
        FROM telemetry AS t
        JOIN processes AS p USING (process_id)
        JOIN switches AS s
          ON t.thread_id = s.next_thread_id
         AND t.observed_at >= s.run_start
         AND t.observed_at < s.run_end
        ORDER BY t.observed_at, t.thread_id
        """,
        tables={
            "telemetry": telemetry,
            "processes": processes,
            "switches": switches,
        },
    )
