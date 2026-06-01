# htrace-core

`htrace-core` 是 Rust TraceStreamer 工作区的基础协议模块。

## 业务职责

- 定义跨 crate 共享的错误类型和 `TraceResult`，统一解析、查询、IO、schema、参数等错误边界。
- 定义 `TraceQueryEngine` trait，抽象 trace 打开、检查、查询和关闭生命周期。
- 定义查询入口使用的请求与返回结构，包括 `TraceInput`、`OpenOptions`、`TraceHandle`、`QueryRequest`、`QueryResult`、`TraceInspection`。
- 提供稳定的 `SCHEMA_VERSION`，让 CLI、Web UI、query crate 和外部调用方使用一致的协议标识。

## 设计边界

- 本 crate 只放稳定、轻量、无业务状态的数据结构和接口。
- 不解析 trace，不持有表 schema，也不直接依赖 Harmony/OpenHarmony 格式细节。
- parser、model、query 中的领域行为不要下沉到本 crate，避免 core 反向知道具体表或具体插件。
