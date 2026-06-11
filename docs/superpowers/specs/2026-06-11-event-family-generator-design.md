# Event Family Generator 设计

## 背景

当前 `build.rs` 已经能从 `sched.proto` 生成 `sched_rows.rs` 和 `sched_table_builders.rs`，但生成器仍然以 sched 为中心命名：`SCHED_PROTO`、`generate_sched_code`、`render_sched_rows`、`render_sched_table_builders`。这让后续接入其他 ftrace event family 时容易复制一套相同字符串生成逻辑。

本次只做生成器内部抽象，不改变已生成的 sched Row、direct table builders、SQL 表名、列名、字段号和运行时行为。

## 要解决的问题

1. 用 `EventFamilySpec` 表达一个事件族的生成配置，让 sched 通过 family generator 生成。
2. 将 sched 专用生成函数改成通用 event family 生成函数。
3. 保持当前输出文件名：`sched_rows.rs` 和 `sched_table_builders.rs`。
4. 保持当前运行时代码不引入 runtime plugin 或 trait object。

## 不做什么

1. 不接入新的事件族，例如 irq、binder、workqueue。
2. 不改变 `sched.proto` 字段内容、字段号或 message 名。
3. 不改变 `hitrace.rs`、`derived.rs` 的业务语义。
4. 不重写 proto parser；当前手写 parser 的风险后续单独处理。

## 设计

新增 build-time 配置：

```rust
struct EventFamilySpec {
    proto_path: &'static str,
    rows_file: &'static str,
    builders_file: &'static str,
    meta_name: &'static str,
    observer_name: &'static str,
    builders_name: &'static str,
}
```

定义当前唯一 family：

```rust
const SCHED_FAMILY: EventFamilySpec = EventFamilySpec {
    proto_path: "proto/ftrace_data/sched.proto",
    rows_file: "sched_rows.rs",
    builders_file: "sched_table_builders.rs",
    meta_name: "SchedEventMeta",
    observer_name: "SchedEventObserver",
    builders_name: "SchedDirectTableBuilders",
};
```

`generate_event_family_code(&SCHED_FAMILY)` 负责读取 proto、解析 message、写出两个生成文件。`render_event_rows` 和 `render_event_table_builders` 接收 `EventFamilySpec`，通过 spec 中的名字生成 `SchedEventMeta`、`SchedEventObserver` 和 `SchedDirectTableBuilders`。

这个抽象是 build-time 的，不影响 runtime 模块边界。生成代码仍然只服务 direct/raw event table，派生表继续通过 observer 在 `derived.rs` 中手写。

## 验证

1. 架构测试检查 `build.rs` 中存在 `EventFamilySpec`、`SCHED_FAMILY`、`generate_event_family_code`、`render_event_rows`、`render_event_table_builders`。
2. 架构测试检查 `build.rs` 不再出现 `generate_sched_code`、`render_sched_rows`、`render_sched_table_builders`。
3. `proto_contract` 继续验证 generated sched rows 和 generated table builders 可用。
4. `hitrace_datasource_query` 继续验证 sched 明细表、`thread_state`、`instant` 查询行为。
5. 全量运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
6. 使用真实 trace 查询核心 sched 表 count。
