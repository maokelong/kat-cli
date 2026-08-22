---
status: accepted
---

# PostgreSQL common 使用 autocommit 且不提供跨查询事务

公共 PostgreSQL common 建立 Psycopg 短连接时使用 `autocommit=True`，不显式执行 `BEGIN`、`COMMIT` 或 `ROLLBACK`。每次查询仍由 PostgreSQL 提供语句级事务与数据快照，但不同 `execute_sql_file()` 或 `execute_sql_text()` 调用之间不保证共享连接、事务或快照；只读边界继续由数据库账号权限保证。首个切片不提供跨查询事务 API。
