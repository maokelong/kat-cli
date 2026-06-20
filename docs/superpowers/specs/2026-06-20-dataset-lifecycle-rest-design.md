# Dataset lifecycle REST 设计

Issue #60 的范围在 #75 和 #77 合入后已经收窄。`POST /v1/datasets` 已经能同步 materialize 本地 dataset；`POST /v1/datasets/queries` 已经能直接按 dataset 执行 SQL，不再要求用户先创建 datasource id。本切片补齐本地 dataset 的最小生命周期闭环：列出、查看、删除。

## 目标

1. 用户可以通过 REST/OpenAPI 发现当前 dataset 根目录下有哪些 dataset。
2. 用户可以查看某个 dataset 的轻量结构，确认它在哪里、有哪些可查询表。
3. 用户可以删除不再需要的本地 dataset；替换语义由删除后重新创建表达。
4. 继续保持 dataset 是本地可重建产物，不把旧数据提升为长期系统状态。

## 不做什么

1. 不实现 `PUT`、`PATCH` 或 replace。
2. 不支持 path dataset 输入或公开 locator 概念。
3. 不在 inspect 中读取 row count、schema 或 preview data。
4. 不实现 `.htrace` 或 Langfuse materialize 阶段内存优化。
5. 不设计 `derived/`、`runs/` 等后续 layout。

## API

新增三个 endpoint：

```text
GET    /v1/datasets?directory=/absolute/path&limit=100&offset=0
GET    /v1/datasets/{datasetName}?directory=/absolute/path
DELETE /v1/datasets/{datasetName}?directory=/absolute/path
```

`directory` 与已有 `dataset.directory` 语义一致：可省略，省略时使用平台默认 dataset 根目录；传入时必须是绝对路径。`datasetName` 复用已有 dataset name 校验，最终 dataset 目录为 `<directory>/<datasetName>`。

List 使用已有分页 envelope：

```json
{
  "data": [
    {
      "name": "my-dataset",
      "directory": "/absolute/path/to/datasets",
      "path": "/absolute/path/to/datasets/my-dataset"
    }
  ],
  "pagination": {
    "limit": 100,
    "offset": 0,
    "totalItems": 1
  }
}
```

Inspect 返回 dataset 和轻量 table metadata：

```json
{
  "data": {
    "dataset": {
      "name": "my-dataset",
      "directory": "/absolute/path/to/datasets",
      "path": "/absolute/path/to/datasets/my-dataset"
    },
    "tables": [
      {
        "name": "langfuse_traces",
        "path": "tables/langfuse_traces.parquet",
        "sizeBytes": 12345
      }
    ]
  }
}
```

Table metadata 只来自 `catalog.json` 和文件 metadata。这里不读取 Parquet schema，不执行 count，不返回 preview rows，避免 inspect 被误用成查询接口。

Delete 成功返回 `204 No Content`，与 datasource delete 保持一致。缺失 dataset 返回 `404 DATASET_NOT_FOUND`。

## 错误边界

| 场景 | HTTP | code |
| --- | --- | --- |
| query 参数结构错误 | 400 | `BAD_REQUEST` |
| `directory` 是相对路径 | 422 | `VALIDATION_FAILED` |
| dataset 根目录不存在 | list 返回空列表 | - |
| dataset 不存在 | 404 | `DATASET_NOT_FOUND` |
| dataset 路径存在但不是目录 | 422 | `VALIDATION_FAILED` |
| catalog 缺失、非法或表文件逃逸根目录 | 422 | `VALIDATION_FAILED` |

删除接口只删除通过 `directory + datasetName` 解析得到的 dataset 目录。它不接受任意 path，也不跟随 symlink 删除外部目标。

## 实现切片

1. 在 `kat-rs-datasource` 暴露最小 dataset inspect helper，复用现有 catalog 校验逻辑，返回逻辑表名、相对路径和文件大小。
2. 在 `kat-rs-daemon` 的 dataset service 中增加 list、inspect、delete。
3. 在 `routes::datasets` 增加 list / inspect / delete 路由，保持统一 error envelope。
4. 更新 OpenAPI schema 和 README endpoint 示例。

## 验证

1. API contract：创建 Langfuse fixture dataset 后，`GET /v1/datasets` 能列出它。
2. API contract：`GET /v1/datasets/{datasetName}` 返回 dataset 和 `langfuse_traces` / `langfuse_observations` 表信息。
3. API contract：`DELETE /v1/datasets/{datasetName}` 返回 204，随后 inspect 返回 `404 DATASET_NOT_FOUND`。
4. OpenAPI contract：`/openapi.json` 包含新增路径、query 参数和响应 schema。
5. 回归：`cargo test --workspace`、clippy、PR guard 和 `git diff --check` 通过。
