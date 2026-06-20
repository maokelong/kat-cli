# dataset 直接查询 REST 设计

## 背景

#75 已把 `.htrace` 和 Langfuse legacy 的本地 dataset materialization 接到 `POST /v1/datasets`。用户现在可以通过 REST 生成本地 Parquet dataset，并拿到 `data.dataset.name`、`directory` 和 `path`。

#60 剩余的下一刀不是再创建一个 server 进程内 datasource 句柄，而是让用户能直接给定 dataset 执行 SQL。已有 `kat-rs-datasource::TraceDatasource::from_dataset(path)` 已能从 catalog 注册 Parquet 表并查询；当前缺口只是 daemon REST 没有暴露这个能力。

同时，现有 datasource query 响应是：

```json
{
  "data": {
    "rows": [],
    "rowCount": 0
  },
  "meta": {}
}
```

项目尚未发布，没有历史兼容债务。趁本次新增 dataset query，应把查询类 API 统一成更适合人和 AI 扫描的响应：先返回 `meta`，再返回 `rowCount`，最后返回行数据 `data`。

## 要解决的问题

1. 支持通过 REST 直接查询已有本地 dataset。
2. 让 dataset query 复用 #75 已确定的 `dataset.name` + 可选 `dataset.directory` 定位方式。
3. 统一 query endpoint 的成功响应结构，避免 datasource query 与 dataset query 形成两套结果形态。
4. 保持错误响应继续使用统一 error envelope。

## 不做什么

1. 不创建 dataset datasourceId，不把 dataset query 接入 server datasource registry。
2. 不实现 dataset list、delete、inspect、replace 或 lifecycle 管理。
3. 不实现 query 结果分页、流式输出、取消或超时控制。
4. 不实现跨请求 dataset catalog cache；每次请求先按 dataset 定位并打开 catalog/Parquet。
5. 不优化 `.htrace` 或 Langfuse materialize 阶段峰值内存。
6. 不引入 `target`、`locator`、完整 `path` 输入或 `state` 字段。

## 方案选择

采用：

```text
POST /v1/datasets/queries
```

请求体：

```json
{
  "dataset": {
    "name": "my-dataset",
    "directory": "/absolute/path/to/datasets"
  },
  "sql": "select count(*) as trace_count from langfuse_traces"
}
```

`dataset.directory` 可省略；省略时使用平台默认 dataset 根目录。传入时必须是绝对路径。定位语义与 `POST /v1/datasets` 一致：最终 dataset 目录是 `<directory>/<name>`。

备选方案及取舍：

1. `POST /v1/datasets/queries`：用户直接给 dataset 和 SQL，概念最少，和 #75 的 dataset 对象连贯。本切片选择它。
2. `POST /v1/datasources` 增加 `source: "DATASET"`：技术上复用现有 datasource query，但会要求用户理解中间 `datasourceId`，把落盘 dataset 和 server 内存句柄混在一起。
3. `POST /v1/queries` 加 `target`：表面通用，但 `target` 会重新引入此前已拒绝的抽象字段，用户不能一眼看懂目标是什么。

## 成功响应

所有 query endpoint 统一返回：

```json
{
  "meta": {
    "elapsedMs": 12,
    "dataset": {
      "name": "my-dataset",
      "directory": "/absolute/path/to/datasets",
      "path": "/absolute/path/to/datasets/my-dataset"
    }
  },
  "rowCount": 1,
  "data": [
    { "trace_count": 1 }
  ]
}
```

datasource query 对应返回：

```json
{
  "meta": {
    "elapsedMs": 12,
    "datasourceId": "ds_xxx"
  },
  "rowCount": 1,
  "data": [
    { "trace_count": 1 }
  ]
}
```

JSON 标准不要求消费者依赖字段顺序；但 Rust DTO、OpenAPI 示例、README 示例都按 `meta`、`rowCount`、`data` 排列。这样 AI 或人类读取响应时可以先确认查询上下文和结果规模，再处理可能很大的 rows。

`data` 是 rows array，不再包一层 `data.rows`。这是破坏性变更，但项目尚无发布兼容承诺，当前修改比后续保留两套 query shape 更简单。

## 错误模型

继续使用现有统一 error envelope：

```json
{
  "error": {
    "code": "VALIDATION_FAILED",
    "message": "...",
    "details": null
  }
}
```

建议状态码：

| HTTP 状态码 | 错误码 | 场景 |
| --- | --- | --- |
| `400` | `BAD_REQUEST` | JSON 结构错误、未知字段、缺少 `dataset`、缺少 `dataset.name`、缺少 `sql`、字段类型错误。 |
| `404` | `DATASET_NOT_FOUND` | resolved dataset 目录不存在。 |
| `422` | `VALIDATION_FAILED` | dataset 名称非法、`dataset.directory` 不是绝对路径、catalog 不存在或结构非法、Parquet 文件缺失或不可读。 |
| `422` | `QUERY_FAILED` | SQL parse、planning 或 execution 失败。 |
| `500` | `INTERNAL` | 非预期内部错误。 |

新增 `DATASET_NOT_FOUND` 只用于明确的“目标 dataset 目录不存在”。如果目录存在但 catalog 或 Parquet 无法注册，属于已有数据结构不合法，返回 `VALIDATION_FAILED`。

## 架构与数据流

`kat-rs-daemon` 增加薄的 dataset query service 或复用现有 dataset service。它只负责：

1. 解析 HTTP DTO。
2. 复用 `DatasetStore` / `DatasetLocator::Name` 把公开 `dataset` 映射到 resolved path。
3. 校验 dataset 目录是否存在。
4. 调用 `TraceDatasource::from_dataset(path)`。
5. 执行 SQL 并组装统一 query response。

数据流：

```text
POST /v1/datasets/queries
  -> routes::datasets::query_dataset
  -> resolve dataset directory or default DatasetStore
  -> DatasetStore::resolve(DatasetLocator::Name(name))
  -> TraceDatasource::from_dataset(resolved path)
  -> TraceDatasource::query_json(sql)
  -> QueryResponseEnvelope
```

本切片不缓存 `TraceDatasource`。这保持 server 无状态、行为可解释，也符合 AGENTS.md 对可重建本地持久化产物的约束：进程不会把历史 dataset 自动恢复成长期可信运行状态。后续如果重复打开 catalog 成本成为真实瓶颈，再以独立 issue 设计内部 cache；cache 不应先成为用户 API 概念。

## OpenAPI 与文档

OpenAPI 增加或调整：

1. `DatasetQueryRequest`
2. `DatasetQueryMeta`
3. `DatasourceQueryMeta`
4. 统一 query success schema，例如 `QueryResponse`
5. `/v1/datasets/queries` 的 `post` path
6. 现有 `/v1/datasources/{datasourceId}/queries` 的成功响应 schema

README 更新：

1. `POST /v1/datasets` 生成 dataset 后，展示 `POST /v1/datasets/queries` 直接查询。
2. 更新 datasource query 示例为 `meta`、`rowCount`、`data`。
3. 明确 dataset query 不创建 datasourceId，不删除或修改 dataset。

## 测试与验证

最小测试集：

1. API contract：先用 `POST /v1/datasets` materialize Langfuse fixture，再删除源文件，用 `POST /v1/datasets/queries` 查询 `langfuse_traces` 成功。
2. 响应 contract：dataset query 成功响应包含顶层 `meta`、`rowCount`、`data`，`meta.dataset` 包含 `name`、`directory`、`path`。
3. 兼容收敛 contract：现有 datasource query 响应也改为顶层 `meta`、`rowCount`、`data`，不再返回 `data.rows` / `data.rowCount`。
4. OpenAPI contract：`/openapi.json` 包含 `/v1/datasets/queries` 和更新后的 query schema。
5. 错误边界：未知字段和缺少字段返回 `400`；dataset 不存在返回 `404 DATASET_NOT_FOUND`；相对 `dataset.directory` 返回 `422 VALIDATION_FAILED`；非法 SQL 返回 `422 QUERY_FAILED`。

提交前验证：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python .github/scripts/test_pr_guard.py
git diff --check
```

## 最小交付切片

1. 新增 `POST /v1/datasets/queries`。
2. 统一 datasource query 与 dataset query 成功响应结构。
3. 新增必要 DTO、OpenAPI schema 和 contract tests。
4. 更新 README。
5. 不实现 dataset lifecycle 和性能优化。
