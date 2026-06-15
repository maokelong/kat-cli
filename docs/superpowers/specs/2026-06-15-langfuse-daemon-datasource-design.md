# Langfuse legacy datasource daemon 设计

## 背景

Langfuse legacy export 由 `observations/*.jsonl.gz` 和 `traces/*.jsonl.gz` 文件组成，单个文件可能达到 2GB+。如果继续沿用当前 CLI 的单次命令生命周期，用户连续执行多条查询时，每次 `kat-rs query --source langfuse --observations-file ... --traces-file ... --sql ...` 都会重新创建 `TraceDatasource`，反复支付文件校验、DataFusion JSONL/GZ 表注册、schema 推断和查询上下文构建成本。

这个问题的根因不是单次加载不够快，而是 datasource 生命周期放在短生命周期 CLI 进程里。长期方向应把 datasource 生命周期提升到本机 daemon，由 CLI、MCP 或调试工具通过 HTTP API 使用同一个已注册 datasource。

当前最新代码中，Langfuse legacy 通过 `TraceDatasource::from_langfuse_legacy(observations_path, traces_path)` 注册两张 DataFusion JSONL/GZ 外部表：`langfuse_observations` 和 `langfuse_traces`。它不把两张表完整转成内存 `RecordBatch`；daemon 第一版复用的是 `SessionContext`、表注册和 datasource handle，查询时仍由 DataFusion 扫描源文件。

## 要解决的问题

本次设计要解决：

1. 为 Langfuse legacy 等大文件 datasource 提供本机常驻生命周期。
2. 让同一个 datasource identity 连续查询时复用 daemon 中已注册的 datasource。
3. 提供可直接用 URL/curl/MCP 调试的 REST API。
4. 让服务端负责输入文件组身份识别，避免信任客户端传入的 size/mtime。
5. 为后续磁盘缓存、内存水位控制和 MCP 接入保留清晰边界。

## 不做什么

1. 不做远程访问、多用户隔离或鉴权；第一版只监听 `127.0.0.1`。
2. 不做 daemon 自动拉起；第一版由用户显式启动和关闭。
3. 不做磁盘缓存。磁盘缓存后续用于内存压力控制，不作为本次冷启动加速手段。
4. 不做 idle timeout、LRU 或内存水位淘汰；第一版只支持显式关闭 datasource。
5. 不做异步 `LOADING` 状态；第一版创建请求同步返回 `READY` 或错误。
6. 不让 `kat-rs query` 包装 daemon；第一版 CLI 只负责 daemon 启停，查询直接走 HTTP API。
7. 不提交大体积真实 Langfuse fixture。

## 方案选择

采用本机 Axum REST daemon：

```text
CLI / MCP / curl
  -> 127.0.0.1 REST API
  -> kat-rs-daemon
  -> DatasourceService
  -> DatasourceRegistry
  -> kat-rs-datasource
```

备选方案及取舍：

1. 批量 SQL 模式实现更小，但不能解决用户反复执行多个 CLI 进程时重复加载的问题，也不能承接 MCP。
2. 磁盘 columnar/materialized cache 可以跨进程复用，但会把第一刀带向缓存格式、失效和落盘成本，偏离生命周期问题。
3. daemon REST API 能直接修正 datasource 生命周期边界，并自然支持后续 MCP 和人工 URL 调试。

## REST API

API 使用资源路径，不使用 RPC 风格路径。

```text
GET    /v1/health
POST   /v1/datasources
GET    /v1/datasources?limit=100&offset=0
GET    /v1/datasources/{datasourceId}
DELETE /v1/datasources/{datasourceId}
POST   /v1/datasources/{datasourceId}/queries
DELETE /v1/server
```

### 创建或复用 datasource

`POST /v1/datasources`

请求按 `source` 做 source-specific schema。Langfuse legacy 请求：

```json
{
  "source": "LANGFUSE_LEGACY",
  "observationsFile": "C:\\abs\\observations.jsonl.gz",
  "tracesFile": "C:\\abs\\traces.jsonl.gz"
}
```

hitrace 请求：

```json
{
  "source": "HITRACE",
  "file": "C:\\abs\\trace.hitrace"
}
```

服务端负责：

1. 校验 `source` 是单值枚举。
2. 按 `source` 校验必需文件字段，拒绝无关字段。
3. canonicalize 每个输入文件路径。
4. 读取每个输入文件 metadata，生成 `DatasourceIdentityKey(source, inputs)`。
5. 命中已加载 datasource 时返回 `200 OK`。
6. 未命中时同步加载，成功后返回 `201 Created`。

返回：

```json
{
  "data": {
    "id": "ds_01J...",
    "source": "LANGFUSE_LEGACY",
    "inputs": [
      {
        "role": "OBSERVATIONS",
        "path": "C:\\abs\\observations.jsonl.gz",
        "sizeBytes": 2147483648,
        "modifiedAt": "2026-06-15T10:12:30.123Z"
      },
      {
        "role": "TRACES",
        "path": "C:\\abs\\traces.jsonl.gz",
        "sizeBytes": 134217728,
        "modifiedAt": "2026-06-15T10:12:31.123Z"
      }
    ],
    "state": "READY",
    "createdAt": "2026-06-15T10:12:31.000Z",
    "lastAccessedAt": "2026-06-15T10:12:31.000Z"
  }
}
```

第一版只有 `READY` 状态。加载失败直接返回错误响应，不创建 `FAILED` 资源。

### 查询 datasource

`POST /v1/datasources/{datasourceId}/queries`

请求：

```json
{
  "sql": "select count(*) as count from langfuse_traces"
}
```

返回：

```json
{
  "data": {
    "rows": [
      {
        "count": 42
      }
    ],
    "rowCount": 1
  },
  "meta": {
    "datasourceId": "ds_01J...",
    "elapsedMs": 12
  }
}
```

查询结果使用 envelope，便于后续添加 schema、阶段耗时、截断信息和资源指标。

### 列出 datasource

`GET /v1/datasources` 支持分页参数 `limit` 和 `offset`。第一版 registry 规模通常很小，但列表接口仍保持稳定分页形态。

返回：

```json
{
  "data": [],
  "pagination": {
    "limit": 100,
    "offset": 0,
    "totalItems": 0
  }
}
```

### 关闭 daemon

`DELETE /v1/server` 是本机 daemon 生命周期控制接口，只监听 `127.0.0.1`，不作为远程管理 API。成功后返回 `202 Accepted`，daemon 在响应写出后触发 graceful shutdown。

返回：

```json
{
  "data": {
    "state": "SHUTTING_DOWN"
  }
}
```

### 错误响应

所有错误使用统一结构：

```json
{
  "error": {
    "code": "DATASOURCE_NOT_FOUND",
    "message": "datasource not found",
    "details": {
      "datasourceId": "ds_bad"
    },
    "requestId": "req_01J..."
  }
}
```

HTTP 状态码约定：

| 状态码 | 场景 |
| --- | --- |
| `400` | JSON 格式错误或参数类型错误 |
| `404` | datasource 不存在 |
| `409` | 资源状态冲突 |
| `422` | source、输入路径或 SQL 语义校验失败 |
| `500` | 服务端内部错误，响应不暴露内部栈 |

## Crate 与模块边界

新增 `kat-rs-daemon` crate，daemon 作为独立产品边界，不把 server 代码放进 CLI。

```text
crates/kat-rs-cli
  src/commands.rs
  只负责 daemon start/stop 命令解析和调用

crates/kat-rs-daemon
  src/lib.rs              // router(state), serve(config)
  src/server.rs           // bind/listener/graceful shutdown
  src/routes.rs           // route composition
  src/routes/health.rs
  src/routes/datasources.rs
  src/routes/queries.rs
  src/state.rs            // AppState
  src/api.rs              // request/response DTO
  src/error.rs            // ApiError -> IntoResponse
  src/service.rs          // DatasourceService
  src/registry.rs         // in-memory registry
  src/loader.rs           // source -> datasource build
  src/config.rs

crates/kat-rs-datasource
  不依赖 axum/http
  只暴露 datasource 构建和查询能力
```

Axum handler 保持薄，只负责 `State`、`Path`、`Json` extraction、调用 service、返回 DTO。canonicalize、stat、load、query、registry 更新都放在 service/registry/loader 层。

`kat-rs-datasource` 保持纯业务库，不引入 Axum、HTTP request/response 或 daemon 生命周期概念。

## Datasource 表示

最新代码中 hitrace 和 Langfuse legacy 都由 `TraceDatasource` 承载：hitrace 构建内存 `MemTable`，Langfuse legacy 注册 DataFusion JSONL/GZ 外部表。因此 daemon 第一版不新增 `LangfuseLegacyDatasource` 类型，也不使用 `Arc<dyn QueryableDatasource>`。

```rust
struct DatasourceEntry {
    source: DatasourceSource,
    datasource: Arc<TraceDatasource>,
    // id, identity, timestamps...
}
```

source-specific 差异只放在 loader 中：

1. `HITRACE` 调用 `TraceDatasource::from_hitrace(file)`。
2. `LANGFUSE_LEGACY` 调用 `TraceDatasource::from_langfuse_legacy(observations_file, traces_file).await`。

后续如果 source 需要不同 query 能力，再评估 enum dispatch 或 trait object。

## Registry 与并发

registry 使用一个内部锁保护互相关联的索引，避免多个 `RwLock<HashMap<...>>` 产生一致性问题。

```text
RegistryInner
  entries: datasource_id -> Arc<DatasourceEntry>
  by_identity: DatasourceIdentityKey -> datasource_id
  inflight: DatasourceIdentityKey -> Arc<LoadSlot>
```

`DatasourceEntry` 保存：

1. datasource id
2. source
3. canonical input files
4. datasource identity
5. `READY` state
6. `TraceDatasource`
7. created_at
8. last_accessed_at

并发创建同一个 datasource identity 时，使用内部 inflight 协调：

1. service 生成 `DatasourceIdentityKey`。
2. registry 查 `by_identity`，命中则直接返回已加载 datasource。
3. 未命中但 `inflight` 存在时，当前请求等待同一个 `LoadSlot`。
4. 未命中且无 `inflight` 时，当前请求插入 `LoadSlot` 并成为 loader。
5. loader 获取 `loadLimiter` permit。
6. loader 用 `tokio::task::spawn_blocking` 执行大文件加载。
7. 加载成功后短暂持有 registry 写锁，插入 `entries` 和 `by_identity`，移除 `inflight`。
8. loader 写入 `LoadSlot` 结果并通知等待者。
9. 等待者复用同一个结果；失败时所有等待者收到同类结构化错误，不重复加载。

同步约束：

1. 不持有 registry 锁跨 `.await`、加载或查询。
2. 不用全局 open mutex 包住完整加载过程。
3. `maxConcurrentLoads` 用 `tokio::sync::Semaphore` 表达，第一版默认可以是 `1`，后续再配置化。
4. query 从 registry 取出 `Arc<DatasourceEntry>` 后释放锁再执行。
5. query 默认并发执行；如果具体 datasource 不满足 `Send + Sync`，实现时退回 entry 级 query mutex。

## CLI 行为

第一版 CLI 只提供 daemon 生命周期命令：

```text
kat-rs daemon start --host 127.0.0.1 --port 0
kat-rs daemon stop
```

`start` 启动 Axum server。默认监听 `127.0.0.1`，端口可配置。第一版以前台进程运行，并支持 Ctrl-C graceful shutdown。

`stop` 向本机 daemon 发送 `DELETE /v1/server`。它只关闭同一台机器上的 daemon，不引入远程控制或自动拉起。

第一版不把 `kat-rs query` 接到 daemon。HTTP API 是主要查询入口，便于用 curl、浏览器工具和 MCP 直接调试。

## 验证计划

API contract 测试：

1. `POST /v1/datasources` 新建 datasource 返回 `201`。
2. 同一输入文件组第二次 `POST /v1/datasources` 返回 `200` 且同一个 id。
3. `GET /v1/datasources` 返回列表。
4. `GET /v1/datasources/{id}` 返回单个资源。
5. `DELETE /v1/datasources/{id}` 后再次 `GET` 返回 `404`。
6. 错误响应统一为 `{ "error": ... }`。

服务端身份测试：

1. request 只传 source-specific 文件路径，例如 `observationsFile` 和 `tracesFile`。
2. 每个输入文件的 size/mtime 来自服务端 stat。
3. 任一输入文件变更后再次 `POST` 得到新的 datasource id。

并发测试：

1. 用 fake loader 注入延迟。
2. N 个并发 `POST /v1/datasources` 同一输入文件组只触发一次 load。
3. 所有成功请求返回同一个 datasource id。
4. load 失败时等待者都拿到同类错误，不重复 load。

查询测试：

1. 创建 datasource 后，`POST /v1/datasources/{id}/queries` 返回 envelope。
2. 不存在 id 返回 `404`。
3. SQL 执行错误返回结构化错误，不把 anyhow/debug 栈直接暴露给 HTTP 客户端。

CLI 测试：

1. `kat-rs daemon start --host 127.0.0.1 --port 0` 能启动。
2. `kat-rs daemon stop` 能关闭本机 daemon。

全量验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 后续扩展

后续独立设计再考虑：

1. MCP client 对接同一套 REST API。
2. `kat-rs query` 自动连接 daemon 或显式 `--daemon`。
3. daemon 自动拉起。
4. 异步 datasource create，增加 `LOADING/FAILED` 状态。
5. 磁盘缓存、spill、内存水位、LRU 和 idle timeout。
6. 远程访问、鉴权和多用户隔离。
