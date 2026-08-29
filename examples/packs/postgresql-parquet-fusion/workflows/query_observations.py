import kat

from kat.pack.datasources.postgresql import PostgreSQLProvider


@kat.workflow(
    name="query-observations",
    title="Query PostgreSQL observations",
    required_tables=[],
    parameters={
        "service": "libpq service name.",
        "database": "Database containing observations.",
        "clock_domain": "Clock domain of observation.observed_at.",
        "start_clock_value": "Inclusive observation window start.",
        "end_clock_value": "Exclusive observation window end.",
    },
)
def query_observations(
    ctx: kat.Context,
    service: str,
    database: str,
    clock_domain: str,
    start_clock_value: int,
    end_clock_value: int,
):
    """直接返回一次 PostgreSQL 查询形成的 eager Table。"""
    del ctx
    clock_domain = clock_domain.strip()
    if not clock_domain:
        raise ValueError("clock_domain must be non-empty")
    if start_clock_value >= end_clock_value:
        raise ValueError(
            "start_clock_value must be less than end_clock_value"
        )

    return PostgreSQLProvider(service=service).query(
        """
        SELECT
            thread_id,
            $3::TEXT AS clock_domain,
            observed_at AS clock_value,
            cpu_usage
        FROM observation
        WHERE observed_at >= $1
          AND observed_at < $2
        ORDER BY observed_at, thread_id
        """,
        database=database,
        params=(start_clock_value, end_clock_value, clock_domain),
    )
