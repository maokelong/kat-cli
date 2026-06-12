# sched direct tables 设计

## 背景

Issue [#25](https://github.com/maokelong/kat-rs/issues/25) 需要补齐 sched ftrace 事件的 protobuf 描述，并让这些事件能在 kat-rs 中查询。当前 PR 只交付 direct tables：每个 sched 事件消息生成一张同名明细表。

Issue [#22](https://github.com/maokelong/kat-rs/issues/22) 中的 `thread_state`、`instant` 等派生表不进入本 PR，后续用单独设计和 PR 实现。

## PR 定位

本 PR 是 sched direct tables 的最小功能验证，不作为 datasource 最终解码架构落地。当前实现只验证 `.htrace -> ftrace-plugin -> sched direct tables -> DataFusion` 这条纵向链路。

当前的 `hitrace.rs` 仍然承担临时编排职责：它同时串联 hitrace/profiler container、ftrace plugin payload 和 Arrow direct table sink。后续架构 PR 应把这些职责拆开：

```text
formats/hitrace  -> .htrace 文件容器、ProfilerPluginData envelope、segment streaming decode
domains/ftrace   -> TracePluginResult、FtraceEvent、common fields、event family 语义
sinks/arrow      -> TraceRecord / direct events 到 Arrow RecordBatch
catalog/query    -> TraceDataset/TableCatalog 注册到 DataFusion
```

因此，本 PR 不新增 `TraceRecord`、`TraceDataset`、`formats/`、`domains/` 或 `sinks/` 目录。相关边界会在后续独立 PR 中设计和迁移。

## 目标

1. 增加 `proto/ftrace_data/sched.proto`，使用 `package kat.hitrace`，格式与 `hitrace.proto` 保持一致。
2. 在 `hitrace.proto` 的 `FtraceEvent` 中引入 sched 事件字段。
3. 通过 build script 从 sched proto 生成 direct table builders。
4. 解析 hitrace 文件时，按 len-prefixed protobuf message streaming decode `ProfilerPluginData`。
5. 将 `ProfilerPluginData` 和 sched direct events 写入 Arrow `RecordBatch`，并注册到 DataFusion。
6. 支持查询 sched direct tables，例如 `sched_switch`、`sched_wakeup`、`sched_blocked_reason`、`sched_migrate_task`。

## 非目标

1. 不实现 `thread_state`、`instant`、`process`、`thread`、`sched_slice`、`raw_event` 等派生表。
2. 不在本 PR 中引入跨事件状态机、线程生命周期推导或 CPU slice 合成逻辑。
3. 不复制 TraceStreamer 的全部 ftrace schema；只添加当前 sched direct tables 需要的消息。
4. 不引入 `prost-reflect` 或运行时 protobuf 字段反射。

## 数据流

```text
hitrace file
  -> profiler section
  -> len-prefixed ProfilerPluginData streaming decode
  -> profiler_plugin_data TableBuilder
  -> ftrace-plugin TracePluginResult
  -> FtraceEvent
  -> SchedDirectTableBuilders
  -> DirectEventTableBuilder
  -> EventRow<SchedXxxFormat>
  -> HitraceTable
  -> DataFusion
```

解析入口保持薄：它只负责读取 section、decode protobuf、把 direct event 推给生成的 builders。sched 事件字段到表的路由由生成代码承担。

## 表行模型

direct table 行由两部分组成：

| 部分 | 来源 | 字段 |
| --- | --- | --- |
| 公共字段 | `FtraceCpuDetailMsg.cpu` 和 `FtraceEvent` | `event_timestamp`, `event_cpu`, `event_tgid`, `event_comm` |
| 消息字段 | `SchedXxxFormat` | 该 sched message 自身字段 |

公共字段只手写一次：

```rust
EventMeta::from_event(cpu, &event)
```

每个 direct event 写入时仍使用通用 wrapper 作为 serde 适配器：

```rust
EventRow<M> {
    #[serde(flatten)]
    meta: EventMeta,
    #[serde(flatten)]
    message: M,
}
```

但 Arrow schema 不再依赖 `EventRow<M>` 的 sample 推导。`DirectEventTableBuilder::new::<M>()` 分别获取两组 fields：

1. 公共字段：由 `EventMeta` 的 schema helper 固定生成。
2. 消息字段：由 `serde_arrow::from_type::<M>()` 从 prost 生成的 `SchedXxxFormat` 推导。

两组 fields 合并后创建 `serde_arrow::ArrayBuilder`。写入时仍把 `EventMeta + M` 包成 `EventRow<M>`，交给 `serde_arrow::ArrayBuilder::push` 负责序列化。

这个方案把公共字段 schema 与消息字段 schema 分离，同时保留 prost 强类型和 serde_arrow 写入路径。它不要求引入运行时 protobuf 反射，也不需要手写 Arrow 原生列 builder。

`ProfilerPluginData` 不使用公共字段，继续使用通用 `TableBuilder<T>` 和 `serde_arrow::from_type::<T>()`。

## 代码生成

`build.rs` 中的 `EventFamilySpec` 描述一个 event family：

| 字段 | 作用 |
| --- | --- |
| `proto_path` | family proto 文件 |
| `builders_file` | 生成 direct table builders |
| `builders_name` | 生成 builder 集合名称 |

生成物：

1. `OUT_DIR/sched_table_builders.rs`：生成 `SchedDirectTableBuilders`，内部持有每张 sched direct table 的 `DirectEventTableBuilder`。
2. `new()`：对每个 sched message 调用 `DirectEventTableBuilder::new::<SchedXxxFormat>("table_name")`。
3. `push_event(cpu, event)`：把 `FtraceEvent` optional 字段路由到对应 direct table builder，并用 `EventMeta::from_event(...)` 补齐公共字段。

不生成 `OUT_DIR/sched_rows.rs`、`SchedEventMeta` 或 `SchedXxxRow`。

## Direct Tables

当前 sched direct tables 与 `sched.proto` 中的 `*Format` message 一一对应。每张表都包含公共字段：

| 字段 | 类型 | 来源 |
| --- | --- | --- |
| `event_timestamp` | `uint64` | `FtraceEvent.timestamp` |
| `event_cpu` | `uint32` | `FtraceCpuDetailMsg.cpu` |
| `event_tgid` | `int32` | `FtraceEvent.tgid` |
| `event_comm` | `string` | `FtraceEvent.comm` |

每张表再追加对应 sched message 的原始字段。例如：

| 表 | 消息 | 关键字段 |
| --- | --- | --- |
| `sched_switch` | `SchedSwitchFormat` | `prev_comm`, `prev_pid`, `prev_prio`, `prev_state`, `next_comm`, `next_pid`, `next_prio` |
| `sched_wakeup` | `SchedWakeupFormat` | `comm`, `pid`, `prio`, `target_cpu` |
| `sched_blocked_reason` | `SchedBlockedReasonFormat` | `pid`, `caller`, `io_wait` |
| `sched_migrate_task` | `SchedMigrateTaskFormat` | `comm`, `pid`, `prio`, `orig_cpu`, `dest_cpu` |

## 验证

1. `serde_arrow_contract` 验证 `EventMeta` fields 能与 `serde_arrow::from_type::<SchedXxxFormat>()` fields 合并，并能通过 `EventRow<M>` 写入扁平 Arrow columns。
2. `proto_contract` 验证 prost 生成的 sched proto 类型、`FtraceEvent` 字段和 generated direct builders。
3. `hitrace_architecture_contract` 验证解析入口只接入 direct tables，生成代码使用 `DirectEventTableBuilder`，且不再生成/include `sched_rows.rs`。
4. `hitrace_datasource_query` 用测试 hitrace bytes 验证 direct sched tables 可通过 DataFusion 查询。
5. 全量验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
