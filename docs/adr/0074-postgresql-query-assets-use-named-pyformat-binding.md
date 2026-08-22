---
status: superseded by ADR-0081
---

# PostgreSQL Query Asset 使用具名 pyformat 绑定

`dialect = "postgresql"` 的 Query Asset 与 Ad Hoc Query 使用 `%(name)s` 表达具名值参数。公共 PostgreSQL common 先要求调用参数名称与 `query.toml` 声明完全一致并校验基础值类型，再把原始 SQL 和独立参数字典交给 Psycopg；它不执行字符串替换、不解析 SQL，也不把参数翻译成另一套占位符。该语法是 KAT PostgreSQL Query Asset 合同而不只是当前驱动偶然行为，未来替换底层驱动时仍需保持兼容或显式重新设计。
