---
status: accepted
---

# Workflow 通过文件路径选择固化 PostgreSQL SQL

公共 PostgreSQL common 的固化 SQL Interface 是一步式 `execute_sql_file(ctx, sql_file_path, parameters)`：Workflow 直接提供具体文件系统路径，common 以 UTF-8 读取该 SQL 文件、使用独立参数映射交给 Psycopg 执行，并把唯一 rowset 转换成 DataFusion DataFrame。分析期生成的 SQL 文本继续通过 `execute_sql_text(ctx, sql_text, parameters)` 执行。两种接口都隐藏 Psycopg connection、cursor 和 Arrow 转换细节，但文件路径是 Workflow 显式拥有的依赖。

首个切片不构建或绑定 ResourceCatalog，不按稳定 source/query 名称查找 SQL，不要求 `source.toml`、`query.toml` 或 `--resource-dir` 才能运行。Data Source Knowledge、Schema 文档和固化 SQL 仍可以组织在 PACK 外的共享目录中并由多个 PACK 引用，入口 Skill 也可以按具体路径读取知识文件；这些目录和辅助 manifest 不形成 KAT Runtime registry 或执行前提。

本决定取代 ADR-0063、ADR-0065、ADR-0069、ADR-0070、ADR-0071 和 ADR-0072 中关于静态 source/query discovery、Query Asset manifest、ResourceCatalog 注入和隐藏物理路径的设计；不改变公共 common 与 PACK 分层、显式 `kat.Context`、进程级 libpq 环境变量或隐藏数据库驱动对象的决定。
