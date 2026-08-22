---
status: accepted
---

# Run 第一版不记录外部 SQL 文件溯源

公共 PostgreSQL common 第一版不扩展 `kat.Context` 或 Run Manifest 来记录 `execute_sql_file()` 使用的绝对路径、SQL 文件内容或内容摘要。Run 继续只记录既有的 PACK、Workflow、实际 Workflow 输入和 Run Output；外部 SQL 文件暂时作为受信任部署内容处理，与 PACK 使用的其他本地文件相同。

若真实审计或复现需求出现，应统一设计可供不同 common 能力使用的“执行依赖摘要”，而不是只为 PostgreSQL 增加专用 Run 字段。本决定不禁止 Workflow 自己把路径声明为普通输入，但 common 不隐式发布或记录它。
