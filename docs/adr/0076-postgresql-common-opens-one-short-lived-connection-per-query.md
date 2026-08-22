---
status: accepted
---

# PostgreSQL common 每次查询建立一个短连接

公共 PostgreSQL common 的 `execute_sql_file()` 和 `execute_sql_text()` 每次调用都根据当前进程的 libpq 环境变量建立一个 Psycopg 连接，在结果完整转换为 Arrow 并交给 `kat.Context` 后立即关闭。连接、cursor、连接池和跨调用缓存不暴露给 Workflow，也不在首个切片中引入执行期资源注册或清理机制；因此同一 Workflow 中的多次查询调用会分别建立连接。若真实环境的测量证明连接建立成为瓶颈，可以在不改变 Workflow 查询接口的前提下另行设计执行期复用。
