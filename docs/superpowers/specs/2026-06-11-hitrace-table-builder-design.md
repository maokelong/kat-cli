# hitrace direct sched 表流式构建设计

## 背景

当前 sched 明细表的路径是先把每个 direct sched event 转成 `Sched*Row`，存入 `Vec<Sched*Row>`，解析完成后再通过 `serde_arrow::to_record_batch` 一次性转成 Arrow `RecordBatch`。这个路径可用，但会让 direct event 表多保留一份行数据，并且 `SchedRows` 同时承担收集和最终建表两件事。

`serde_arrow` 已经提供 `ArrayBuilder`，可以从同一套 `SchemaLike::from_type::<Row>` 推导出的 Arrow fields 创建 builder，并支持逐条 `push(row)`。这允许 direct sched 明细表在 decode 时逐条进入 Arrow builder，保留 serde_arrow 的 schema 推导能力，又避免马上自生成底层 typed Arrow builders。

## 要解决的问题

1. direct sched 明细表从 `Vec<Sched*Row>` 改为基于 `serde_arrow::ArrayBuilder` 的流式表构建。
2. 保留现有 SQL 表名、列名、列类型和查询行为。
3. 保留 build 生成的 `Sched*Row`，让它继续作为 serde_arrow schema 和单行序列化边界。
4. 降低后续接入更多 direct event 表时的内存压力和中间行持有时间。

## 不做什么

1. 不生成 typed Arrow builder，例如 `UInt64Builder` / `StringBuilder` 逐字段 append。
2. 不修改 `thread_state` / `instant` 派生表语义。
3. 不把 direct sched 表改成 Perfetto 风格的 `ftrace_event + args` 模型。
4. 不扩大到非 sched ftrace events。

## 设计

新增一个轻量 `TableBuilder<T>`：

```rust
struct TableBuilder<T> {
    name: &'static str,
    builder: serde_arrow::ArrayBuilder,
    _row: PhantomData<T>,
}
```

`TableBuilder<T>` 负责：

1. 用 `Vec::<FieldRef>::from_type::<T>(TracingOptions::default())` 推导 schema。
2. 用 `serde_arrow::ArrayBuilder::from_arrow(&fields)` 初始化空表 builder。
3. 在 decode 阶段通过 `push(row)` 写入一行。
4. 在解析结束时通过 `into_record_batch()` 产出 `HitraceTable`。

`SchedRows` 仍然作为当前 sched direct 表收集器存在，但字段类型从 `Vec<Sched*Row>` 改为 `TableBuilder<Sched*Row>`。这样本次切片只改变落地方式，不同时改事件族模块边界。

## 派生表边界

`thread_state` 和 `instant` 当前留在 `src/hitrace/derived.rs`。它们暂时继续使用行向量和 `table_from_rows`：

- `instant` 后续可以改为 streaming builder。
- `thread_state` 需要先修语义，因为 `dur` 需要等下一状态出现才能确定，不能简单 append 后回写。

## 验证

1. 新增架构测试，要求 direct sched 表不再以 `Vec<Sched*Row>` 存在，并存在 `TableBuilder`。
2. 现有 datasource 测试验证 sched 明细表、空表、`thread_state`、`instant` 查询结果不变。
3. `proto_contract` 验证生成 Row 仍包含公共事件列和 message 字段。
4. 全量运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
5. 使用真实 trace 查询 `sched_switch`、`sched_wakeup`、`thread_state`、`instant` count。
