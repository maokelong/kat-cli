# sched direct tables 设计

## 背景

Issue [#25](https://github.com/maokelong/kat-rs/issues/25) 需要补齐 sched ftrace 事件的 protobuf 描述，并让这些事件能在 kat-rs 中查询。当前 PR 只交付 direct tables：每个 sched 事件消息生成一张同名明细表。Issue [#22](https://github.com/maokelong/kat-rs/issues/22) 中的 `thread_state`、`instant` 等派生表先不进入本 PR，后续用单独设计和 PR 实现。

## 目标

1. 增加 `proto/ftrace_data/sched.proto`，使用 `package kat.hitrace`，格式与 `hitrace.proto` 保持一致。
2. 在 `hitrace.proto` 的 `FtraceEvent` 中引入 sched 事件字段。
3. 通过 build script 从 sched proto 生成 direct row structs 和 direct table builders。
4. 解析 hitrace 文件时，按 len-prefixed protobuf message streaming decode `ProfilerPluginData`。
5. 将 `ProfilerPluginData` 和 sched direct rows 写入 Arrow `RecordBatch`，并注册到 DataFusion。
6. 支持查询 sched direct tables，例如 `sched_switch`、`sched_wakeup`、`sched_blocked_reason`、`sched_migrate_task`。

## 非目标

1. 不实现 `thread_state`、`instant`、`process`、`thread`、`sched_slice`、`raw_event` 等派生表。
2. 不在本 PR 中引入跨事件状态机、线程生命周期推导或 CPU slice 合成逻辑。
3. 不复刻 TraceStreamer 的全部 ftrace schema；只添加当前 sched direct tables 需要的消息。
4. 不为未来 event family 预留复杂运行时插件系统；只保留 build-time event family generator。

## 数据流

```text
hitrace file
  -> profiler section
  -> len-prefixed ProfilerPluginData streaming decode
  -> profiler_plugin_data TableBuilder
  -> ftrace-plugin TracePluginResult
  -> FtraceEvent
  -> SchedDirectTableBuilders
  -> HitraceTable
  -> DataFusion
```

解析入口保持薄：它只负责读取 section、decode protobuf、把 direct event 推给生成的 builders。sched 事件字段到表的映射由生成代码承担。

## 代码生成

`build.rs` 中的 `EventFamilySpec` 描述一个 event family：

| 字段 | 作用 |
| --- | --- |
| `proto_path` | family proto 文件 |
| `rows_file` | 生成 direct row structs |
| `builders_file` | 生成 direct table builders |
| `meta_name` | 生成事件公共元信息结构 |
| `builders_name` | 生成 builder 集合名称 |

生成物：

1. `OUT_DIR/sched_rows.rs`：为每个 `*Format` message 生成一个 `*Row`，字段包含事件公共元信息和 message 自身字段。
2. `OUT_DIR/sched_table_builders.rs`：生成 `SchedDirectTableBuilders`，内部持有每张 sched direct table 的 `TableBuilder<T>`，并在 `push_event(cpu, event)` 中把 `FtraceEvent` optional 字段写入对应表。

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

1. `proto_contract` 验证 prost 生成的 sched proto 类型、`FtraceEvent` 字段和 generated direct builders。
2. `hitrace_architecture_contract` 验证解析入口只接入 direct tables，不接入派生表逻辑。
3. `hitrace_datasource_query` 用测试 hitrace bytes 验证 direct sched tables 可通过 DataFusion 查询。
4. 全量验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
