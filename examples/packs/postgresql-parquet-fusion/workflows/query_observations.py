import kat

from kat.pack.datasources.postgresql import PostgreSQLProvider


@kat.workflow(
    name="query-observations",
    title="Query PostgreSQL observations",
    required_tables=[],
    parameters={
        "service": "libpq service name.",
        "database": "Database containing observations.",
        "start_ns": "Inclusive observation window start.",
        "end_ns": "Exclusive observation window end.",
    },
)
def query_observations(
    ctx: kat.Context,
    service: str,
    database: str,
    start_ns: int,
    end_ns: int,
):
    """直接返回一次 PostgreSQL 查询形成的 eager Table。"""
    del ctx
    if start_ns >= end_ns:
        raise ValueError("start_ns must be less than end_ns")

    return PostgreSQLProvider(service=service).query(
        """
        SELECT thread_id, observed_at, cpu_usage
        FROM observation
        WHERE observed_at >= $1
          AND observed_at < $2
        ORDER BY observed_at, thread_id
        """,
        database=database,
        params=(start_ns, end_ns),
    )
