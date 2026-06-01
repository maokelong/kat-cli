# htrace-query

`htrace-query` 是 Rust TraceStreamer 的 SQL 查询层。

## 业务职责

- 将 `ParsedTrace` 中的 Arrow `RecordBatch` 注册到 DataFusion。
- 执行 SQL，并返回列、行、状态和查询统计信息。
- 将 Arrow 查询结果转换为 JSON 友好的 `QueryResult`，支持空结果、截断标记和常见基础数据类型。
- 提供 `HtraceDataFusionEngine`，实现 `TraceQueryEngine` 的 open/inspect/query/close 生命周期。
- 为 CLI、Web UI、对比报告工具提供同一套查询能力。

## 设计边界

- 不解析 trace，不修改模型数据。
- 查询层应兼容空表，保证部分插件未产出数据时 SQL 仍能给出稳定结果或清晰错误。
- DataFusion 注册、SQL 执行和结果序列化属于本 crate；表 schema 和业务字段解释属于 `htrace-model` 与 parser。
