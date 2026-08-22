from pathlib import Path

import kat
from kat.common.sql import postgresql


@kat.workflow(
    name="query-postgresql-file",
    title="Query PostgreSQL from SQL File",
    required_tables=[],
)
def query_postgresql_file(ctx: kat.Context):
    """执行 PACK 中固化的 PostgreSQL SQL 文件并发布结果集。"""
    sql_file_path = Path(__file__).resolve().parents[1] / "queries" / "smoke.sql"
    return {
        "postgresql_result": postgresql.execute_sql_file(
            ctx,
            sql_file_path=sql_file_path,
        )
    }
