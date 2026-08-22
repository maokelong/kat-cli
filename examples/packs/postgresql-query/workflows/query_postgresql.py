import kat
from kat.common.sql import postgresql


@kat.workflow(
    name="query-postgresql",
    title="Query PostgreSQL",
    required_tables=[],
    parameters={
        "sql": "原样交给 PostgreSQL 执行，并且必须恰好返回一个小型结果集。"
    },
)
def query_postgresql(ctx: kat.Context, sql: str):
    """在 PostgreSQL 原样执行 SQL，并发布唯一的小型结果集。"""
    return {
        "postgresql_result": postgresql.execute_sql_text(
            ctx,
            sql_text=sql,
        )
    }
