from pathlib import Path

import kat

from kat import dataprovider as dp
from kat.pack.datasources.postgresql import PostgreSQLProvider


@kat.workflow(
    name="fuse-observations",
    description="融合 PostgreSQL 观测数据与本地线程部署信息。",
    parameters={
        "service": "libpq service name.",
        "telemetry_database": "Database containing observations.",
        "control_database": "Database containing process metadata.",
        "placement_root": "Directory containing thread_placement.parquet.",
        "clock_domain": "Clock domain of observation.observed_at.",
        "start_clock_value": "Inclusive observation window start.",
        "end_clock_value": "Exclusive observation window end.",
    },
    guide="workflows/fuse-observations.md",
)
def fuse_observations(
    ctx: kat.Context,
    service: str,
    telemetry_database: str,
    control_database: str,
    placement_root: str,
    clock_domain: str,
    start_clock_value: int,
    end_clock_value: int,
):
    """顺序查询两个 Database，再与本地 Parquet Catalog 显式融合。"""
    del ctx
    clock_domain = clock_domain.strip()
    if not clock_domain:
        raise ValueError("clock_domain must be non-empty")
    if start_clock_value >= end_clock_value:
        raise ValueError(
            "start_clock_value must be less than end_clock_value"
        )

    postgresql = PostgreSQLProvider(service=service)
    telemetry = postgresql.query(
        """
        SELECT
            o.thread_id,
            r.process_id,
            $3::TEXT AS clock_domain,
            o.observed_at AS clock_value,
            AVG(o.cpu_usage)::DOUBLE PRECISION AS cpu_usage
        FROM observation AS o
        JOIN thread_registry AS r USING (thread_id)
        WHERE o.observed_at >= $1
          AND o.observed_at < $2
        GROUP BY o.thread_id, r.process_id, o.observed_at
        """,
        database=telemetry_database,
        params=(start_clock_value, end_clock_value, clock_domain),
    )

    processes = postgresql.query(
        """
        SELECT process_id, process_name
        FROM process_registry
        """,
        database=control_database,
    )

    placement = dp.open(
        tables={
            "thread_placement": (
                Path(placement_root) / "thread_placement.parquet"
            )
        }
    )

    fusion = dp.DataFusionProvider(
        tables={
            "telemetry": telemetry,
            "processes": processes,
        },
        catalog=placement,
    )
    return fusion.query(
        """
        SELECT
            t.thread_id,
            t.process_id,
            p.process_name,
            t.clock_domain,
            t.clock_value,
            placement.cpu,
            t.cpu_usage
        FROM telemetry AS t
        JOIN processes AS p USING (process_id)
        JOIN thread_placement AS placement USING (thread_id)
        ORDER BY t.clock_value, t.thread_id
        """
    )
