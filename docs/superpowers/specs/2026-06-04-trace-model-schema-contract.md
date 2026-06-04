# trace-model schema contract 设计

## 背景

`kat-rs-datasource` 后续会通过 parser 解析 protobuf htrace，并通过 query 层对解析后的表执行 SQL 查询。在 parser 和 query 接入之前，`trace-model` 需要先提供稳定的表契约，避免后续代码各自定义表名、字段类型和可空性。

本 PR 是 datasource 首次拆分上库中的 trace-model schema contract 切片。它只定义“当前已验证 protobuf htrace 能产出数据的表”，不引入 builder、parser、query 业务逻辑。

## 目标

- 在 `trace-model` 中定义已验证表清单。
- 让 `trace.v1.json` 成为 schema contract 的唯一来源。
- 从 JSON manifest 派生 Arrow `SchemaRef` 和表名查询能力。
- 提供 `schema_for_table` 作为表名到 schema 的查询入口。
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

这些表只在 `schema/trace.v1.json` 中维护一次。Rust 侧通过 `manifest.rs` 读取内嵌 JSON，再由 `schema.rs` 转成 Arrow schema，由 `tables.rs` 派生表名清单。

## 关键设计

### 1. JSON manifest 是唯一来源

`trace.v1.json` 保存表名、字段名、字段类型和 nullable 规则。Rust 不再手写每张表的字段列表，也不再维护重复的 `TRACE_TABLE_NAMES` 常量。

运行时使用 `include_str!` 内嵌 JSON，并通过 `OnceLock` 懒加载解析结果。这里的 `OnceLock` 只缓存不可变 schema manifest，不缓存 trace 文件、解析结果、查询结果或用户数据，因此不改变 MVP 阶段“不做业务 cache”的约束。

### 2. 配置扩展字段不需要生成 `.rs`

如果后续只是为某张表增加一个已支持类型的字段，例如 `Utf8`、`Int64`、`UInt64`、`UInt32`、`Int32`、`Boolean`、`Float64`，只需要更新 `trace.v1.json`。Rust schema 会自动从 manifest 生成新的 Arrow 字段。

只有在新增一种 Arrow 数据类型时，才需要修改 `TraceDataType` 枚举和 `schema.rs` 中的类型映射。这属于类型系统能力扩展，不是每个字段都要生成或手写 Rust 代码。

### 3. schema contract 先于 builder

本 PR 只定义表契约，不创建 `TraceTables` 或 RecordBatch builder。这样 schema review 可以聚焦在字段名、类型和 nullable 规则上。builder 后续 PR 只需要实现这些已确认 schema 的数据构建逻辑。

### 4. 表清单只包含已验证表

之前源码中存在一些未映射或未验证表的残留，例如 `symbols`、`cpu_usage`、`diskio`、`sys_mem_measure`、`js_heap_*`。这些表不进入本次 contract。

原因是 datasource 的目标是数据准确和查询快速。暴露无法由 parser 稳定产出的表，会让 CLI/Web UI/query 层展示空能力，也会误导使用方。

### 5. 依赖保持最小

本 PR 只新增 schema contract 所需依赖：

- `arrow-schema`：构造 Arrow `SchemaRef`。
- `serde`：反序列化 JSON manifest。
- `serde_json`：解析内嵌 manifest。

不引入 `arrow-array`、`prost`、`datafusion`，因为 builder、parser、query 还没有进入本 PR。

## 文件职责

- `crates/kat-rs-datasource/crates/trace-model/schema/trace.v1.json`
  - 保存 JSON schema manifest。
  - 作为 schema contract 的唯一数据来源。

- `crates/kat-rs-datasource/crates/trace-model/src/manifest.rs`
  - 定义 manifest 反序列化结构。
  - 通过 `include_str!` 内嵌 `trace.v1.json`。
  - 通过 `OnceLock` 懒加载不可变 manifest。

- `crates/kat-rs-datasource/crates/trace-model/src/schema.rs`
  - 从 manifest 生成 Arrow `SchemaRef`。
  - 提供 `schema_for_table`。

- `crates/kat-rs-datasource/crates/trace-model/src/tables.rs`
  - 从 manifest 派生 `table_names`。
  - 提供 `is_trace_table`。

- `crates/kat-rs-datasource/crates/trace-model/tests/schema_contract.rs`
  - 验证只暴露 19 张已验证表。
  - 验证每张表都能查到 schema。
  - 验证 Arrow schema 与 JSON manifest 逐列一致。
  - 验证未映射表不会暴露 schema。

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
