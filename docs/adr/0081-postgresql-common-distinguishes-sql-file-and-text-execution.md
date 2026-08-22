---
status: accepted
---

# PostgreSQL common 明确区分 SQL 文件与文本执行

公共 PostgreSQL common 使用 `execute_sql_file(ctx, sql_file_path, parameters)` 执行指定文件中的固化 SQL，使用 `execute_sql_text(ctx, sql_text, parameters)` 执行调用者直接提供的 SQL 文本。两个名称明确表达 SQL 来源，共享“必须恰好返回一个 rowset 并生成 DataFusion DataFrame”的执行合同。

两个接口的 `parameters` 都是可选的字符串键映射，SQL 使用 Psycopg 具名 pyformat `%(name)s` 占位符。common 不解析 SQL、不自行比对占位符，也不限制 Psycopg 可以适配的参数值类型；它把 SQL 与参数映射分离传给 Psycopg，由驱动报告缺失参数、占位符错误和不支持的值类型。任何接口都不得执行字符串替换。该决定取代 ADR-0074 中依赖 Query Asset manifest 的参数校验合同。
