# 生成 SchedDirectTableBuilders 设计

## 背景

上一轮已经把 direct sched 明细表从 `Vec<Sched*Row>` 改成 `serde_arrow::ArrayBuilder` 逐行构建，但 `hitrace.rs` 里仍手写了 `SchedRows`、所有 sched 表字段、`new()` 初始化、`push_event()` 路由和 `into_tables()` 长列表。这和“像 proto/schema 一样生成”的目标不一致。

`serde_arrow` 能从一个 Row 类型推导 Arrow schema，也能逐行 append，但它不能知道 `FtraceEvent` 中哪个 optional sched 字段应该进入哪张 SQL 表。这个事件清单和字段名已经存在于 `proto/ftrace_data/sched.proto`，所以 direct sched 表集合和路由应继续由 build 阶段生成。

## 要解决的问题

1. `hitrace.rs` 不再手写 `SchedRows`、direct sched 表字段列表、direct event 路由和 direct 表 `into_tables` 列表。
2. `build.rs` 基于 `sched.proto` 生成 `SchedDirectTableBuilders`，和现有 `Sched*Row` 使用同一份 `ProtoMessage` 清单。
3. direct sched 表仍使用 `serde_arrow::ArrayBuilder` 逐行构建，保持当前 SQL 表名、列名、列类型和查询行为。
4. `thread_state` / `instant` 派生语义继续留在 `src/hitrace/derived.rs`，不塞进生成器。

## 不做什么

1. 不生成 typed Arrow builder，例如逐字段 `UInt64Builder` / `StringBuilder`。
2. 不改变 `thread_state` / `instant` 的语义。
3. 不把 sched direct 表改成 Perfetto 的 `ftrace_event + args` 模型。
4. 不扩大到非 sched ftrace events。

## 设计

新增生成文件 `OUT_DIR/sched_table_builders.rs`，并在 `src/lib.rs` 中作为 `sched_table_builders` 模块 include。

生成内容包括：

1. `SchedEventObserver` trait：为每个 sched Row 生成一个默认 no-op 方法，例如 `observe_sched_switch(&mut self, row: &SchedSwitchRow)`。生成代码只负责通知，不理解派生表业务。
2. `SchedDirectTableBuilders` struct：为每个 direct sched 表持有一个 `TableBuilder<Sched*Row>`。
3. `SchedDirectTableBuilders::new()`：初始化所有 direct 表 builder。
4. `SchedDirectTableBuilders::push_event(cpu, event, observer)`：从 `FtraceEvent` 中检查每个 sched optional field，构造对应 Row，先通知 observer，再 append 到 direct 表。
5. `SchedDirectTableBuilders::into_tables()`：按 `sched.proto` 顺序输出所有 direct sched `HitraceTable`。

`TableBuilder<T>` 从 `hitrace.rs` 抽到 `src/hitrace/table_builder.rs`，由 `hitrace` 模块以 `pub(crate)` 暴露给生成模块使用。

`derived.rs` 新增 `DerivedTables`，内部持有 `ThreadStateBuilder` 和 `Vec<InstantRow>`，并实现 `SchedEventObserver`：

1. `observe_sched_switch` 更新 `thread_state`。
2. `observe_sched_wakeup` / `observe_sched_wakeup_new` / `observe_sched_waking` 更新 `instant`。
3. `into_tables()` 输出 `thread_state` 和 `instant` 两张派生表。

`hitrace.rs` 只保留流程编排：

1. 创建 `SchedDirectTableBuilders` 和 `DerivedTables`。
2. decode ftrace event 时调用 `sched_tables.push_event(cpu, event, &mut derived_tables)`。
3. 结束后合并 direct 表和派生表。

## 验证

1. 架构测试要求 `hitrace.rs` 不再包含 `struct SchedRows`、`sched_switch: TableBuilder<SchedSwitchRow>` 和 direct Row 构造长列表。
2. 架构测试读取 `OUT_DIR/sched_table_builders.rs`，验证生成了 `SchedDirectTableBuilders`、`SchedEventObserver` 和 direct 表字段。
3. `proto_contract` 增加生成 builders 的 smoke test，确认可以用 observer 接收 `sched_switch` 并输出 direct 表。
4. 现有 datasource 测试继续验证 sched 明细表、空表、`thread_state`、`instant` 查询行为。
5. 全量运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
6. 使用真实 trace 查询 `sched_switch`、`sched_wakeup`、`thread_state`、`instant` count。
