# htrace-web-ui

`htrace-web-ui` 是 Rust TraceStreamer 的本地浏览器查询界面。

## 业务职责

- 启动本地 HTTP 服务，加载一个 trace 文件并在内存中持有解析结果。
- 提供 `/api/inspect`，返回 trace 元信息、时间范围、clock domain、表名、列信息和行数。
- 提供 `/api/query`，接收 SQL 并返回 `htrace-query` 的 JSON 查询结果。
- 提供单页前端界面，用于浏览表、查看统计、编写 SQL、检查查询结果和调试解析输出。

## 设计边界

- UI 不复制 parser、model 或 query 的业务逻辑。
- 新表展示应优先通过 inspect/query 接口读取，避免前端硬编码解析规则。
- 本模块面向本地调试和人工检查，不负责持久化 trace 数据。
