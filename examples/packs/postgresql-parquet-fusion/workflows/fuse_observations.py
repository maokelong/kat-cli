from pathlib import Path

import kat

from kat.pack.helpers.datasources import parquet, postgresql


@kat.workflow(
    name="fuse-observations",
    title="Fuse PostgreSQL observations with local scheduling",
    required_tables=[],
    parameters={
        "profile": "libpq service name.",
        "telemetry_database": "Database containing observations.",
        "control_database": "Database containing process metadata.",
        "trace_root": "Directory containing sched_switch.parquet.",
        "start_ns": "Inclusive observation window start.",
        "end_ns": "Exclusive observation window end.",
    },
)
def fuse_observations(
    ctx: kat.Context,
    profile: str,
    telemetry_database: str,
    control_database: str,
    trace_root: str,
    start_ns: int,
    end_ns: int,
):
    """Localize two databases and one Parquet catalog, then fuse the results."""
    if start_ns >= end_ns:
        raise ValueError("start_ns must be less than end_ns")

    postgresql.provider(
        ctx,
        profile=profile,
        database=telemetry_database,
    ).query(
        """
        SELECT
            o.thread_id,
            r.process_id,
            o.observed_at,
            o.cpu_usage
        FROM observation AS o
        JOIN thread_registry AS r USING (thread_id)
        WHERE o.observed_at >= $1
          AND o.observed_at < $2
        """,
        params=(start_ns, end_ns),
        name="telemetry",
    )

    postgresql.provider(
        ctx,
        profile=profile,
        database=control_database,
    ).query(
        """
        SELECT process_id, process_name
        FROM process_registry
        """,
        name="processes",
    )

    parquet.provider(
        ctx,
        tables={
            "sched_switch": Path(trace_root) / "sched_switch.parquet",
        },
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
        name="switches",
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
        """
    )
