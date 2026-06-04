# trace-model 表契约与构建闭环设计

## 背景

`kat-rs-datasource` 的核心目标是让 protobuf htrace 数据能够被准确解析，并以稳定、可查询的表结构暴露给 CLI、后续 Web UI 和查询层。为了避免 parser、builder、query 各自维护一份表结构，`trace-model` 需要先建立一个清晰的业务契约：什么表可以暴露、字段是什么、字段顺序和类型由谁决定、业务数据为空时如何表现。

本 PR 是 trace-model 的最小闭环切片，只接入已经明确需要的 `trace_bounds` 表。它同时提供表契约、由契约派生的 Arrow schema，以及对应的 RecordBatch builder。后续新增表时，必须按同样方式把“表契约 + 数据构建 + 测试”作为一个完整原子能力一起进入。

## 设计目标

- 只暴露当前已经确认的 protobuf htrace 表能力。
- 让 JSON 表契约成为字段名、字段顺序、字段类型和 nullable 规则的唯一来源。
- 支持 `contracts/tables/**/*.json` 的匹配式注册，避免维护集中式表清单。
- 由契约动态派生 Arrow `SchemaRef`，不在 Rust 代码里重复字段契约。
- builder 只负责 typed row 到 Arrow array 的业务数据转换。
- 结果中只包含真实有行数据的表，无数据的注册表不生成 `RecordBatch`。
- 不引入 parser、query、Web UI 或非 protobuf trace 格式能力。

## 非目标

- 不暴露未映射、未验证或当前不需要的表。
- 不提供动态 `HashMap<String, Value>` 行写入 API。
- 不引入 `build.rs` 代码生成。
- 不接入 bytrace/ftrace text parser。
- 不缓存 trace 文件解析结果或 SQL 查询结果。

## 核心模型

表契约按“一张表一个 JSON”组织：

```text
trace-model/
  contracts/
    tables/
      trace_bounds.json
```

每个 JSON 文件描述一张表的业务字段。`trace-model` 运行时通过嵌入目录匹配所有 `contracts/tables/**/*.json` 文件，解析出表契约集合，再由契约派生 schema 和表名查询能力。这里的 `OnceLock` 只缓存不可变契约元数据，不缓存用户 trace 数据，因此不改变 MVP 阶段“不做业务 cache”的约束。

当前只注册一张表：

```text
trace_bounds
```

字段为：

```text
trace_id      Utf8   non-null
start_ts      Int64  nullable
end_ts        Int64  nullable
clock_domain  Utf8   non-null
```

## 代码结构

```text
crates/kat-rs-datasource/crates/trace-model/
  contracts/tables/
    trace_bounds.json
  src/
    contract.rs
    tables.rs
    builders/
      mod.rs
      batch.rs
      trace_bounds.rs
```

职责划分：

- `contract.rs`
  - 加载并解析 `contracts/tables/**/*.json`。
  - 提供表契约、表名查询和 schema 派生能力。
  - 负责字段类型到 Arrow 类型的映射。
- `tables.rs`
  - 定义 trace-model 对外返回的运行时数据结构。
  - 让上层可以按表名获取已构建的 `RecordBatch`。
- `builders/batch.rs`
  - 提供契约驱动的通用 `RecordBatch` 组装能力。
  - 根据表契约校验字段缺失、额外字段、重复字段和类型不匹配。
  - 拒绝 0 行 `RecordBatch`，空数据由上层 builder 直接跳过。
- `builders/trace_bounds.rs`
  - 定义 `trace_bounds` 的 typed row 和 builder。
  - 只保留业务数据到 Arrow array 的转换逻辑。

## 业务规则

新增表时必须满足：

- 新增一个 `contracts/tables/<table>.json`。
- 新增对应 typed row 和 builder。
- builder 输出必须通过通用 batch 组装逻辑校验。
- 测试必须覆盖契约加载、schema 派生、字段顺序和空数据场景。
- 如果 parser 还不能稳定产出这张表，则不应注册这张表。

如果业务数据为空但契约存在，模型层不返回这张表。这表示“系统支持这张表，但当前 trace 没有数据”。如果契约不存在，则表示“系统当前不支持这张表”，不应该对外暴露。

## 验证

本 PR 需要通过：

```text
cargo test -p trace-model --locked
cargo check --workspace --locked
cargo test --workspace --locked
git diff --check origin/main...HEAD
PR guard
```

验证重点：

- 只注册 `trace_bounds`。
- schema 完全由 JSON 契约派生。
- 未注册表不会返回 schema。
- builder 输出字段顺序遵循契约。
- 缺失、额外、类型不匹配字段会失败。
- 0 行 batch 组装会失败。
- 已注册表没有业务数据时不会进入结果集合。

## 后续演进

PR04 以后不再先上一个庞大的全量 schema，再单独补 builder。每个后续 PR 应该选择一个小的业务表组，例如进程/线程基础表，把契约、builder 和测试一起提交。这样每次入库都是一个可验证、可使用的原子能力。
