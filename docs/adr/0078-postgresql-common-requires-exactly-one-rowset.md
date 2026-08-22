---
status: accepted
---

# PostgreSQL common 要求任意 SQL 恰好产生一个 rowset

公共 PostgreSQL common 将调用者提供的 SQL 原样交给 Psycopg，不解析、不设置语句白名单，也不自动增加 `LIMIT`。一次 `execute_sql_file()` 或 `execute_sql_text()` 调用最终必须恰好产生一个表格结果集；没有 rowset 或产生多个 rowset 都明确失败。唯一 rowset 必须至少包含一列，且列名非空并互不重复。SQL 可以包含不产生 rowset 的语句，其实际能力由 PostgreSQL 只读账号权限约束。
