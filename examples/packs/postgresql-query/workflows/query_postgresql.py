import kat
import psycopg
import pyarrow as pa


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
    result_description = None
    result_rows = None

    with psycopg.connect() as connection:
        with connection.cursor() as cursor:
            cursor.execute(sql)
            for _ in cursor.results():
                if cursor.description is None:
                    continue
                if result_description is not None:
                    raise ValueError("PostgreSQL SQL must return exactly one rowset")
                result_description = tuple(cursor.description)
                result_rows = cursor.fetchall()

    if result_description is None or result_rows is None:
        raise ValueError("PostgreSQL SQL did not return a rowset")
    if not result_description:
        raise ValueError("PostgreSQL rowset must contain at least one column")

    column_names = [column.name for column in result_description]
    if any(not name for name in column_names):
        raise ValueError("PostgreSQL rowset contains an empty column name")
    if len(set(column_names)) != len(column_names):
        raise ValueError(
            "PostgreSQL rowset contains duplicate column names; use unique aliases"
        )

    schema = pa.schema(
        [pa.field(name, pa.string(), nullable=True) for name in column_names]
    )
    arrays = [
        pa.array(
            [
                None if row[column_index] is None else str(row[column_index])
                for row in result_rows
            ],
            type=pa.string(),
        )
        for column_index in range(len(column_names))
    ]
    table = pa.Table.from_arrays(arrays, schema=schema)
    return {"postgresql_result": ctx.from_arrow(table)}
