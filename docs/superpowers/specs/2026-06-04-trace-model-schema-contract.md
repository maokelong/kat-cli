# trace-model schema contract 设计

## 背景

`kat-rs-datasource` 后续会通过 parser 解析 protobuf htrace，并通过 query 层对解析后的表执行 SQL 查询。在 parser 和 query 接入之前，`trace-model` 需要先提供稳定的表契约，避免后续代码各自定义表名、字段类型和可空性。

本 PR 是 datasource 首次拆分上库中的 trace-model schema contract 切片。它只定义“当前已验证 protobuf htrace 能产出数据的表”，不引入 builder、parser、query 业务逻辑。

## 目标

- 在 `trace-model` 中定义已验证表清单。
- 为每张已验证表提供 Arrow `SchemaRef`。
- 提供 `schema_for_table` 作为表名到 schema 的查询入口。
- 添加 `trace.v1.json` schema manifest，方便 review 和后续跨语言/工具消费。
- 通过测试约束未映射表不应暴露 schema。
- 只引入 schema contract 所需依赖，避免把后续 parser/query 依赖提前带入。

## 非目标

- 不实现 RecordBatch builder。
- 不实现 protobuf htrace parser。
- 不实现 SQL query。
- 不暴露未映射、未验证或当前 trace 中没有数据的表。
- 不接入 bytrace 或 ftrace text 格式。

## 暴露表范围

当前只暴露 19 张 protobuf htrace 已验证有数据的表：

- `trace_metadata`
- `trace_bounds`
- `process`
- `thread`
- `sched_slice`
- `thread_state`
- `raw_event`
- `raw`
- `instant`
- `irq`
- `measure`
- `measure_filter`
- `cpu_measure_filter`
- `dma_fence`
- `data_dict`
- `args`
- `callstack`
- `process_measure`
- `process_measure_filter`

这些表同时出现在：

- `TRACE_TABLE_NAMES`
- `schema_for_table`
- `schema/trace.v1.json`
- `tests/schema_contract.rs`

## 关键设计

### 1. schema contract 先于 builder

本 PR 只定义表契约，不创建 `TraceTables` 或 RecordBatch builder。这样 schema review 可以聚焦在字段名、类型和 nullable 规则上。builder 后续 PR 只需要实现这些已确认 schema 的数据构建逻辑。

### 2. 表清单只包含已验证表

之前源码中存在一些未映射或未验证表的残留，例如 `symbols`、`cpu_usage`、`diskio`、`sys_mem_measure`、`js_heap_*`。这些表不进入本次 contract。

原因是 datasource 的目标是数据准确和查询快速。暴露无法由 parser 稳定产出的表，会让 CLI/Web UI/query 层展示空能力，也会误导使用方。

### 3. Rust schema 与 JSON manifest 同步

Rust 侧通过 `schema.rs` 提供 Arrow schema，JSON 侧通过 `trace.v1.json` 提供可 review 的结构化清单。两者表达同一组 19 张表。

后续如果新增表，必须同时更新 Rust schema、JSON manifest 和 schema contract 测试。

### 4. 依赖保持最小

本 PR 只新增 `arrow-schema`：

- 不需要 `arrow-array`，因为还没有 RecordBatch。
- 不需要 `serde`，因为 JSON manifest 只是静态 contract 文件。
- 不需要 `prost`，因为 parser 还没有进入本 PR。
- 不需要 `datafusion`，因为 query 还没有进入本 PR。

## 文件职责

- `crates/kat-rs-datasource/crates/trace-model/src/tables.rs`
  - 定义 `TRACE_TABLE_NAMES`
  - 提供 `table_names`
  - 提供 `is_trace_table`

- `crates/kat-rs-datasource/crates/trace-model/src/schema.rs`
  - 定义每张表的 Arrow `SchemaRef`
  - 提供 `schema_for_table`

- `crates/kat-rs-datasource/crates/trace-model/schema/trace.v1.json`
  - 保存 JSON schema manifest
  - 作为 review 和工具消费的结构化 contract

- `crates/kat-rs-datasource/crates/trace-model/tests/schema_contract.rs`
  - 验证只暴露 19 张已验证表
  - 验证每张表都能查到 schema
  - 验证未映射表不会暴露 schema

## 验证

本地验证命令：

```text
cargo test -p trace-model --locked
cargo check --workspace --locked
cargo test --workspace --locked
git diff --check origin/main...HEAD
```

PR guard 验证：

```text
python .github/scripts/pr_guard.py --event <event.json> --base origin/main --head HEAD --repo .
```

预期结果：

- trace-model 测试通过。
- workspace check/test 通过。
- PR guard 通过。
- PR diff 不超过 large change 门禁。
- 未映射表只允许出现在拒绝测试中，不允许出现在生产 schema 或 JSON manifest 中。

## 后续 PR

后续 builder PR 应基于本 contract 实现 RecordBatch builder。parser PR 应只向这些已暴露表写入数据。query/datasource/CLI/Web UI 应只展示 contract 中定义的表，直到真实 trace 证明需要新增更多表。

