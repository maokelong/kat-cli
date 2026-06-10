# sched.proto 接入设计

## 背景

Issue [#25](https://github.com/maokelong/kat-rs/issues/25) 要求按 `types/plugins/ftrace_data` 清单逐步覆盖 htrace protobuf datasource。本次只完成清单中的 `sched.proto`，让 datasource 不再依赖仓库内手写的 `SchedSwitchFormat` 缩水结构，而是从本地上游仓 `D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto` 接入调度事件 proto。

现有 kat-rs 已经有最小 htrace 链路：读取 `ProfilerPluginData`，识别 `ftrace-plugin`，解码 `TracePluginResult.ftrace_cpu_detail.event.sched_switch_format`，并暴露 `sched_switch` SQL 表。这个切片的重点是替换 proto 来源，不扩大 SQL 表面。

## 要解决的问题

1. 将上游 `sched.proto` 纳入 `kat-rs-datasource` 的 prost 生成流程，并生成 sched 相关 Rust 类型。
2. 保持当前 `sched_switch` 解码路径可用，`sched_switch` 表继续暴露 `prev_comm`、`prev_pid`、`prev_prio`、`prev_state`、`next_comm`、`next_pid`、`next_prio`。
3. 用测试证明生成类型来自上游 sched proto，并证明 datasource 仍能从 ftrace payload 中读出 `sched_switch`。
4. 用 issue 指定的真实 trace 路径执行一次 SQL 查询，给 PR 留下实际验证证据。

## 不做什么

1. 不接入 `types/plugins/ftrace_data/default`。
2. 不一次性接入 `ftrace_event.proto` 的全部 oneof 分支。
3. 不新增除 `sched_switch` 之外的 SQL 表。
4. 不整理 `sched_wakeup`、`sched_stat_runtime` 等其他 sched 事件语义。
5. 不提交真实 trace fixture，也不把本地 `D:\项目\data\...htrace` 加入仓库。

## 上游 proto 来源

本次使用：

```text
D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto
```

该文件包含 `SchedBlockedReasonFormat`、`SchedKthreadStopFormat`、`SchedMigrateTaskFormat`、`SchedSwitchFormat`、`SchedWakeupFormat` 等 sched 事件消息。当前生产查询只消费 `SchedSwitchFormat`，但接入时保留整个 `sched.proto` 文件，避免继续维护缩水版 sched 消息。

为了让现有 `TracePluginResult` 解码链路继续最小可用，`hitrace.proto` 仍只描述当前已验证的外层结构和 `FtraceEvent.sched_switch_format` 分支。完整 `ftrace_event.proto` 的全量 oneof 接入留给 issue #25 中的 `ftrace_event.proto` 清单项。

## 设计

`crates/kat-rs-datasource/proto/hitrace.proto` 保留 `kat.hitrace` package 和外层 hitrace 消息：

```text
ProfilerPluginData
TracePluginResult
FtraceCpuDetailMsg
FtraceEvent
```

新增 `crates/kat-rs-datasource/proto/ftrace_data/sched.proto`，内容来自上游 `sched.proto`。`hitrace.proto` import 该文件，并让 `FtraceEvent.sched_switch_format` 使用上游 `SchedSwitchFormat` 类型。

`build.rs` 编译 `hitrace.proto` 与 `ftrace_data/sched.proto`，并继续只为 Arrow 行转换需要的类型派生 serde：

```text
ProfilerPluginData
SchedSwitchFormat
```

`src/hitrace.rs` 的解码逻辑保持直白：

```text
ProfilerPluginData.data
  -> TracePluginResult
  -> FtraceCpuDetailMsg.event
  -> FtraceEvent.sched_switch_format
  -> sched_switch RecordBatch
```

如果 prost 生成的 `SchedSwitchFormat` 模块路径因 no-package proto 变化，生产代码只调整 import 路径，不新增运行时 descriptor 或动态 protobuf。

## 测试

1. `crates/kat-rs-datasource/tests/proto_contract.rs` 增加 sched proto 契约：能构造并 round-trip `SchedBlockedReasonFormat`，同时确认 `SchedSwitchFormat` 字段保持当前 SQL 表需要的 schema。
2. `crates/kat-rs-datasource/tests/hitrace_datasource_query.rs` 保持并运行 `query_extracts_sched_switch_from_ftrace_plugin_result`，证明 ftrace payload 解码路径还能产生 `sched_switch` 表。
3. `crates/kat-rs-cli/tests/query_e2e.rs` 保持并运行 `query_prints_sched_switch_fields`，证明 CLI 查询路径不变。
4. 全量运行 `cargo test --workspace` 和 `cargo clippy --workspace --all-targets -- -D warnings`。
5. 对真实 trace 执行：

```powershell
cargo run -p kat-rs-cli -- query --source hitrace --file 'D:\项目\data\hiprofiler-wechat-coldstart-smartperf-20260523-182338.htrace' --sql 'select count(*) as count from sched_switch'
```

## 最小交付

1. 新增上游来源的 `ftrace_data/sched.proto`。
2. 更新 protobuf build 配置，使 sched 类型参与生成。
3. 更新 `hitrace.proto`，删除手写 `SchedSwitchFormat`，改用 imported sched 类型。
4. 更新必要测试，覆盖新增 sched proto 类型和现有 `sched_switch` 查询链路。
5. 在新分支提交并创建 PR，PR 说明包含 issue #25 checklist 项、SQL 表变化、端到端测试、真实 trace 查询、workspace test 和 clippy 结果。
