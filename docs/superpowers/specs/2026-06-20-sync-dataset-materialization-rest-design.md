# 同步 dataset materialization REST 设计

## 背景

Issue #60 承接 #59 的本地列式 dataset 后续切片。当前 `kat-rs-datasource` 已经具备 dataset resolver、Parquet writer、catalog reader，以及 `.htrace` 和 Langfuse legacy 到本地 dataset 的 materialize 内核；server/REST 仍只暴露内存态 datasource 创建和查询。

PR #62 已将用户可见业务功能面收敛到 REST/OpenAPI。因此 dataset materialization 不应再通过 CLI 业务命令暴露，而应成为 server 资源。第一刀先把现有 materializer 接到 REST API，建立最小用户入口，不同时引入异步任务、替换、删除、inspect 或 datasource-from-dataset 查询。

## 要解决的问题

1. 通过 REST/OpenAPI 触发现有 `.htrace` 和 Langfuse legacy dataset materialization。
2. 通过明确的 `dataset` 对象表达输出 dataset，通过 `input` 对象表达输入来源。
3. 成功后返回可验证的 dataset 信息，失败时沿用统一 error envelope。
4. 复用 `DatasetStore` / `DatasetLocator` 执行内部解析，但保持 dataset layout、catalog、Parquet 写入和校验逻辑仍由 `kat-rs-datasource` 拥有。

## 不做什么

1. 不实现异步 job、后台队列、进度、取消、重试或历史状态查询。
2. 不实现 `GET /v1/datasets`、`GET /v1/datasets/{id}`、`DELETE /v1/datasets/{id}` 或 inspect。
3. 不实现替换、append、multi-source merge 或 row-level provenance。
4. 不新增 CLI dataset 业务命令。
5. 不在本切片优化 `.htrace` materialize 峰值内存；当前仍可复用 `ArrowSink::finish()` 聚合路径。
6. 不在 REST 第一版暴露完整 dataset path 作为输入；显式 path dataset 继续作为 datasource crate 内核能力。
7. 不把真实大数据性能记录作为本同步 REST 入口切片的验收条件。

## 方案选择

采用同步阻塞 `POST /v1/datasets`。

备选方案及取舍：

1. 同步阻塞创建：实现最小，直接复用现有 materializer，调用方在 HTTP 响应返回时即可知道 dataset 是否完整写入；缺点是大 Langfuse 输入会长时间占用连接。
2. 异步 job：更适合大输入、进度和取消，但会引入进程内 job registry、状态模型、错误留存和 OpenAPI 面，超过第一刀需要。
3. 只写 OpenAPI/SDD 不实现执行：风险最低，但不能给 #60 增加可用能力。

本切片选择 1。同步语义是当前最小可交付切片；如果真实输入证明连接占用不可接受，再以独立 issue 设计异步 materialization resource。

## REST resource

新增：

```text
POST /v1/datasets
```

请求分为输出 dataset 和输入 source 两部分：

```json
{
  "dataset": {
    "name": "my-dataset"
  },
  "input": {
    "source": "HITRACE",
    "file": "/path/to/input.htrace"
  }
}
```

Langfuse legacy 可以指定自定义 dataset 根目录：

```json
{
  "dataset": {
    "name": "my-dataset",
    "directory": "/path/to/datasets"
  },
  "input": {
    "source": "LANGFUSE_LEGACY",
    "observationsFile": "/path/to/observations.jsonl.gz",
    "tracesFile": "/path/to/traces.jsonl.gz"
  }
}
```

`dataset.name` 是必填字段。需要写入 default dataset 时显式传入 `"name": "default"`，避免客户端漏传目标时意外写入 default dataset。`name` 继续使用 datasource crate 现有名称校验，拒绝空字符串、`.`、`..` 和路径分隔符。

`dataset.directory` 是可选字段。省略时使用平台默认 dataset 根目录；传入时必须是绝对路径，表示 named dataset 的父目录，最终 dataset 目录为 `<directory>/<name>`。第一版 HTTP DTO 不接受完整 dataset `path` 作为输入，避免同时支持 `name`、`directory`、`path` 三套定位方式。`DatasetLocator` 只作为 daemon 到 datasource crate 的内部类型。

成功返回 `201 Created`：

```json
{
  "data": {
    "dataset": {
      "name": "my-dataset",
      "directory": "/resolved/path/to/datasets",
      "path": "/resolved/path/to/datasets/my-dataset"
    }
  }
}
```

`dataset.directory` 是 resolved dataset 根目录，`dataset.path` 是最终 dataset 目录。返回 path 是为了让本机用户能看到实际落盘位置；后续 API 仍应优先用 `dataset.name` 加可选 `dataset.directory` 定位 dataset，而不是要求用户解析或拼接 path。同步 `201` 响应只表示本次 materialize 已完成、当前 catalog 可读，不表示 server 记录了可恢复的长期状态。响应不返回 `state`，也不返回 `createdAt`，因为当前 dataset 不写 manifest，server 不能在未来稳定恢复这个时间。响应不声明 table list、row count、source provenance、catalog version 或 manifest。

## 接口契约表

请求体：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `dataset` | `DatasetLocation` | 是 | 要创建的输出 dataset。 |
| `input` | `DatasetSourceInput` | 是 | 用于 materialize 的输入来源。 |

`DatasetLocation`：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `name` | string | 是 | named dataset 名称；传 `"default"` 表示默认 dataset。 |
| `directory` | string | 否 | named dataset 父目录；省略时使用平台默认 dataset 根目录；传入时必须是绝对路径。 |

`DatasetSourceInput`：

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `source` | enum | 是 | `HITRACE` 或 `LANGFUSE_LEGACY`。 |
| `file` | string | `HITRACE` 必填 | `.htrace` 输入文件路径。 |
| `observationsFile` | string | `LANGFUSE_LEGACY` 必填 | Langfuse observations JSONL GZIP 文件路径。 |
| `tracesFile` | string | `LANGFUSE_LEGACY` 必填 | Langfuse traces JSONL GZIP 文件路径。 |

成功响应：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `data.dataset.name` | string | 创建的 dataset 名称。 |
| `data.dataset.directory` | string | resolved dataset 根目录。 |
| `data.dataset.path` | string | resolved dataset 完整目录。 |

错误与排查：

| HTTP 状态码 | 错误码 | 场景 | 排查建议 |
| --- | --- | --- | --- |
| `400` | `BAD_REQUEST` | JSON 结构错误、未知字段、缺少 `dataset`、缺少 `dataset.name`、缺少 `input` 或 source 变体字段不匹配。 | 按 OpenAPI schema 修正请求体字段和类型。 |
| `409` | `CONFLICT` | 目标 dataset 已存在。 | 换一个 `dataset.name` / `dataset.directory`；删除或替换能力留给后续 lifecycle resource。 |
| `422` | `VALIDATION_FAILED` | 输入文件不可解析、不是文件、metadata 读取失败、dataset 名称非法、`dataset.directory` 不是绝对路径、source 读取或 Parquet 写入失败。 | 确认本机路径可读写、dataset 名称合法、目录为绝对路径。 |
| `500` | `INTERNAL` | materializer panic、任务取消或并发 limiter 关闭。 | 查看 server 日志；如果可复现，应作为实现缺陷修复。 |

## 架构与数据流

`kat-rs-daemon` 新增薄的 `DatasetService`。它负责 HTTP DTO、输入文件校验、把公开 `dataset` 映射到内部 `DatasetStore` / `DatasetLocator::Name`、并发限制和错误映射；不复制 catalog、layout 或 Parquet 细节。

数据流：

```text
POST /v1/datasets
  -> routes::datasets::create_dataset
  -> DatasetService::create
  -> resolve source input files
  -> resolve dataset directory or default DatasetStore
  -> DatasetStore::resolve(DatasetLocator::Name(name))
  -> materialize_*_dataset(source files, resolved path)
  -> DatasetDto
```

`.htrace` 调用 `materialize_hitrace_dataset(file, dataset_path)`。Langfuse legacy 调用 `materialize_langfuse_legacy_dataset(observations_file, traces_file, dataset_path)`。

server 可沿用现有 datasource load limiter 的保守默认值，限制同时 materialize 数量，避免多个大输入同时放大内存。该 limiter 只限制本进程并发，不承诺跨进程锁，也不把旧 dataset 当作 server 运行状态。

## 错误模型

继续使用现有统一 error envelope。具体状态码、错误码、场景和排查建议以接口契约表为准。

目标 dataset 已存在时新增 `CONFLICT` 错误码，并返回 `409 CONFLICT`。这是资源创建冲突，不应被当前实现里缺少 conflict 码的事实降级成 validation error。新增 `CONFLICT` 不改变既有 endpoint 的响应语义，只扩展统一 error envelope 的错误码集合。

## OpenAPI 与文档

OpenAPI 增加：

1. `CreateDatasetRequest`
2. `DatasetLocation`
3. `DatasetSourceInput`
4. `DatasetDto`
5. `/v1/datasets` 的 `post` path

README 只需在实现 PR 中加入最小 curl 示例和边界说明：同步请求会等待 materialize 完成；目标已存在失败；删除 datasource 不删除 dataset；list/delete/inspect 仍是后续能力。

## 测试与验证

最小测试集：

1. API contract：`POST /v1/datasets` materialize Langfuse fixture，返回 `201`，响应包含 `dataset.name`、`dataset.directory` 和 `dataset.path`，resolved path 存在。
2. OpenAPI contract：`/openapi.json` 包含 `/v1/datasets` 和新增 schema。
3. 源文件独立性：materialize 后删除或移动源 fixture，再用 `TraceDatasource::from_dataset` 查询 dataset 成功。
4. 错误边界：未知字段返回 `400`；缺少 `dataset.name` 或 `input` 返回 `400`；目标 dataset 已存在返回 `409`；相对 dataset directory 和非法 source file 返回 `422`。

提交前验证：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

本切片不要求真实大数据 RSS 记录。`.htrace` 分批 flush 和 Langfuse materialize 阶段内存继续留给 #60 的后续性能切片。

## 最小交付切片

1. 新增同步 `POST /v1/datasets`。
2. 新增 `DatasetService` 和相关 DTO/OpenAPI schema。
3. 复用现有 datasource crate materializer 和 resolver。
4. 增加 contract tests。
5. 更新 README 的最小 REST 示例和边界说明。
