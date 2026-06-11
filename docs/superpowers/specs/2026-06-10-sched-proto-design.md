# hitrace sched 事件表设计

## 背景

Issue [#25](https://github.com/maokelong/kat-rs/issues/25) 要求按 `types/plugins/ftrace_data` 清单逐步覆盖 htrace protobuf datasource。本 PR 只接入其中的 `sched.proto`：解码 htrace 中的 sched ftrace events，并把每类 sched event 暴露为可查询 SQL 明细表。

Issue [#22](https://github.com/maokelong/kat-rs/issues/22) 提到 `thread_state`、`instant` 属于从事件流生成的派生表。本 PR 在 sched 明细表基础上补齐最小 sched 派生模型：`process`、`thread`、`thread_state`、`instant`、`sched_slice` 和 `raw_event`。

## 要解决的问题

1. 将上游 `sched.proto` 纳入 `kat-rs-datasource` 的 prost 生成流程。
2. 在 `hitrace.proto` 的 `FtraceEvent` 中补齐 sched message 字段，使 htrace 解码路径能识别 sched 事件。
3. 为 `sched.proto` 中每个 sched message 建表，表名使用 snake_case 事件名，例如 `sched_blocked_reason`、`sched_migrate_task`、`sched_switch`、`sched_wakeup`。
4. 从 `sched.proto` 生成 sched 明细 Row、direct table builder 和事件路由，避免在 `hitrace.rs` 手写重复 schema 与长分发表。
5. direct sched 明细表在 decode 时写入 `serde_arrow::ArrayBuilder`，结束后产出 Arrow `RecordBatch`。
6. 从 sched 事件生成最小 `process`、`thread`、`thread_state`、`instant`、`sched_slice` 和 `raw_event` 派生表。

## 不做什么

1. 不接入 `types/plugins/ftrace_data/default`。
2. 不一次性接入 `ftrace_event.proto` 的全部非 sched 分支。
3. 不复刻 trace_streamer 的完整 data_dict、arg_set、binder runnable、memory/native hook 计数等语义。
4. 不引入 issue #22 提到的通用 YAML lifecycle 配置引擎。
5. 不提交真实 trace fixture，也不把本地 `D:\项目\data\...htrace` 加入仓库。
6. 不生成逐字段 typed Arrow builder，例如 `UInt64Builder` / `StringBuilder`。
7. 不新增 C++ TraceStreamer 的 `raw` 表；本次只新增 Rust rewrite 语义下的 `raw_event` 辅助表。

## Proto 来源

本次参考本地 trace_streamer：

```text
D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto
D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\ftrace_event.proto
```

`sched.proto` 放入：

```text
crates/kat-rs-datasource/proto/ftrace_data/sched.proto
```

`ftrace_event.proto` 不整体复制；只把 sched message 字段和 tag 写入 kat-rs 的 `hitrace.proto`，避免把非 sched 的 300+ 分支带入本次 PR。

## 明细表契约

所有 sched 明细表都包含事件公共元数据。若公共列与 message 字段同名，则公共列加 `event_` 前缀，保留 message 字段原名：

| 公共列 | 类型 | 说明 |
| --- | --- | --- |
| `event_timestamp` | `uint64` | `FtraceEvent.timestamp` |
| `event_cpu` | `uint32` | 所属 `FtraceCpuDetailMsg.cpu` |
| `event_tgid` | `int32` | `FtraceEvent.tgid` |
| `event_comm` | `string` | `FtraceEvent.comm` |

每张表再追加对应 proto message 字段：

| 表 | message | 字段 |
| --- | --- | --- |
| `sched_blocked_reason` | `SchedBlockedReasonFormat` | `pid`, `caller`, `io_wait` |
| `sched_kthread_stop` | `SchedKthreadStopFormat` | `comm`, `pid` |
| `sched_kthread_stop_ret` | `SchedKthreadStopRetFormat` | `ret` |
| `sched_migrate_task` | `SchedMigrateTaskFormat` | `comm`, `pid`, `prio`, `orig_cpu`, `dest_cpu` |
| `sched_move_numa` | `SchedMoveNumaFormat` | `pid`, `tgid`, `ngid`, `src_cpu`, `src_nid`, `dst_cpu`, `dst_nid` |
| `sched_pi_setprio` | `SchedPiSetprioFormat` | `comm`, `pid`, `oldprio`, `newprio` |
| `sched_process_exec` | `SchedProcessExecFormat` | `filename`, `pid`, `old_pid` |
| `sched_process_exit` | `SchedProcessExitFormat` | `comm`, `pid`, `prio` |
| `sched_process_fork` | `SchedProcessForkFormat` | `parent_comm`, `parent_pid`, `child_comm`, `child_pid` |
| `sched_process_free` | `SchedProcessFreeFormat` | `comm`, `pid`, `prio` |
| `sched_process_wait` | `SchedProcessWaitFormat` | `comm`, `pid`, `prio` |
| `sched_stat_blocked` | `SchedStatBlockedFormat` | `comm`, `pid`, `delay` |
| `sched_stat_iowait` | `SchedStatIowaitFormat` | `comm`, `pid`, `delay` |
| `sched_stat_runtime` | `SchedStatRuntimeFormat` | `comm`, `pid`, `runtime`, `vruntime` |
| `sched_stat_sleep` | `SchedStatSleepFormat` | `comm`, `pid`, `delay` |
| `sched_stat_wait` | `SchedStatWaitFormat` | `comm`, `pid`, `delay` |
| `sched_stick_numa` | `SchedStickNumaFormat` | `pid`, `tgid`, `ngid`, `src_cpu`, `src_nid`, `dst_cpu`, `dst_nid` |
| `sched_swap_numa` | `SchedSwapNumaFormat` | `src_pid`, `src_tgid`, `src_ngid`, `src_cpu`, `src_nid`, `dst_pid`, `dst_tgid`, `dst_ngid`, `dst_cpu`, `dst_nid` |
| `sched_switch` | `SchedSwitchFormat` | `prev_comm`, `prev_pid`, `prev_prio`, `prev_state`, `next_comm`, `next_pid`, `next_prio` |
| `sched_wait_task` | `SchedWaitTaskFormat` | `comm`, `pid`, `prio` |
| `sched_wake_idle_without_ipi` | `SchedWakeIdleWithoutIpiFormat` | `cpu` |
| `sched_wakeup` | `SchedWakeupFormat` | `comm`, `pid`, `prio`, `success`, `target_cpu` |
| `sched_wakeup_new` | `SchedWakeupNewFormat` | `comm`, `pid`, `prio`, `success`, `target_cpu` |
| `sched_waking` | `SchedWakingFormat` | `comm`, `pid`, `prio`, `success`, `target_cpu` |

## 生成与运行时边界

`build.rs` 使用 `EventFamilySpec` 描述事件族生成配置，当前唯一 family 是 sched：

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

`generate_event_family_code(&SCHED_FAMILY)` 从 `sched.proto` 生成两类文件：

1. `OUT_DIR/sched_rows.rs`：生成 `Sched*Row`、表名常量和从 event/message 构造 row 的逻辑。Row 类型继续作为 serde_arrow schema 推导和单行序列化边界。
2. `OUT_DIR/sched_table_builders.rs`：生成 `SchedEventObserver` 和 `SchedDirectTableBuilders`。builder 负责 direct sched 表集合、event optional field 路由、通知 observer，以及最终输出 `HitraceTable`。

运行时代码的边界：

1. `src/hitrace/table_builder.rs` 提供轻量 `TableBuilder<T>`，内部使用 `serde_arrow::ArrayBuilder` 逐行 append。
2. `src/hitrace/derived.rs` 实现 `SchedEventObserver`，承载 sched 派生表语义。
3. `src/hitrace.rs` 只保留流程编排：解码 `ProfilerPluginData.data`，遍历 ftrace event，调用 `SchedDirectTableBuilders::push_event`，最后合并 direct 表与派生表。

这让 sched 明细表的 schema 和路由跟随 proto 生成，派生表业务语义仍显式保留在手写代码里。

## 派生表契约

`process` 是 sched 事件可推导出的最小进程表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | `uint32` | 等同 `ipid` |
| `ipid` | `uint32` | 内部进程 id |
| `pid` | `int32` | 系统进程 id |
| `name` | `string or null` | 进程名，优先来自主线程名或 exec/fork 信息 |
| `start_ts` | `uint64 or null` | 首次确认该进程的时间 |
| `switch_count` | `uint64` | 本 PR 固定为 0 |
| `thread_count` | `uint64` | 关联线程数 |
| `slice_count` | `uint64` | 本 PR 固定为 0 |
| `mem_count` | `uint64` | 本 PR 固定为 0 |

`thread` 是 sched 事件可推导出的最小线程表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | `uint32` | 等同 `itid` |
| `itid` | `uint32` | 内部线程 id |
| `tid` | `int32` | 系统线程 id |
| `name` | `string or null` | 线程名 |
| `start_ts` | `uint64 or null` | 首次确认该线程的时间 |
| `end_ts` | `uint64 or null` | 线程结束时间，本 PR 只在明确 exit/free 时补齐 |
| `ipid` | `uint32 or null` | 所属内部进程 id |
| `is_main_thread` | `bool or null` | `tid == process.pid` 时为 true；未知进程时为空 |
| `switch_count` | `uint64` | 作为 next 线程运行的次数 |

`thread_state` 是从 `sched_switch` 生成的最小区间表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `ts` | `uint64` | 状态开始时间 |
| `dur` | `uint64 or null` | 到下一次状态变化的持续时间 |
| `cpu` | `uint32 or null` | 运行态所在 CPU；非运行态为空 |
| `itid` | `uint32` | 内部线程 id，可关联 `thread.itid` |
| `tid` | `int32` | 系统线程 id |
| `pid` | `int32 or null` | 所属系统进程 id |
| `state` | `string` | `Running` 或 `prev_state:<value>` |
| `comm` | `string` | 线程名 |

生成规则：

1. 每个 `sched_switch` 产生一行 `next_pid` 的 `Running` 状态。
2. 每个 `sched_switch` 产生一行 `prev_pid` 的 `prev_state:<prev_state>` 状态。
3. 同一 `itid` 上一行状态在新状态开始时补齐 `dur`。
4. 最后一段没有结束事件时 `dur = null`。

`instant` 是从唤醒事件生成的最小瞬时表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `ts` | `uint64` | 事件时间 |
| `name` | `string` | `sched_wakeup`、`sched_wakeup_new` 或 `sched_waking` |
| `ref` | `uint32` | 被唤醒线程的 `itid` |
| `wakeup_from` | `uint32` | 触发唤醒线程的 `itid` |
| `ref_type` | `string` | 固定为 `itid` |
| `value` | `double` | 当前固定为 `0.0` |

`sched_slice` 是从 `sched_switch` 生成的 CPU running 区间表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `id` | `uint64` | slice 行 id |
| `ts` | `uint64` | 运行开始时间 |
| `dur` | `uint64 or null` | 运行持续时间 |
| `ts_end` | `uint64 or null` | `ts + dur` |
| `cpu` | `uint32` | CPU id |
| `itid` | `uint32` | 运行线程 `itid` |
| `ipid` | `uint32 or null` | 运行线程所属 `ipid` |
| `end_state` | `string or null` | 被切出时的状态 |
| `priority` | `int32` | next 线程优先级 |
| `arg_setid` | `uint32 or null` | 本 PR 固定为空 |

`raw_event` 是辅助表，用 JSON 保存 sched 事件原始摘要：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `ts` | `uint64` | 事件时间 |
| `cpu` | `uint32` | CPU id |
| `tid` | `int32 or null` | 事件相关系统线程 id |
| `event_name` | `string` | sched 事件名 |
| `payload_json` | `string or null` | 事件关键字段 JSON |

## 数据流

```text
ProfilerPluginData.data
  -> TracePluginResult
  -> FtraceCpuDetailMsg(cpu)
  -> FtraceEvent(timestamp, tgid, comm, sched message fields)
  -> generated SchedDirectTableBuilders
  -> sched_* direct tables
  -> DerivedTables(process, thread, thread_state, instant, sched_slice, raw_event)
  -> DataFusion MemTable
```

## 验证

1. `proto_contract` 验证上游 `SchedBlockedReasonFormat` 与 `SchedSwitchFormat` 能生成并 round-trip，并验证 generated rows/builders 可用。
2. datasource 测试构造最小 `.htrace`，覆盖 `sched_blocked_reason`、`sched_migrate_task`、`sched_switch`、`sched_wakeup`、`sched_wakeup_new`、`sched_waking` 等表可查询。
3. datasource 测试验证未出现的 sched 表也能注册并返回 `count = 0`。
4. datasource 测试验证 `process`、`thread`、`thread_state`、`instant`、`sched_slice` 和 `raw_event` 从 sched 事件生成。
5. 架构测试约束 `hitrace.rs` 保持薄编排，sched rows/builders 由 build 生成，`build.rs` 使用 event family generator。
6. 全量运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
7. 对真实 trace 执行 `sched_switch`、`sched_wakeup`、`process`、`thread`、`thread_state`、`instant`、`sched_slice`、`raw_event` 等 count 查询，并可导出包含所有可解析表的 `.db` 文件。

## 最小交付

1. 上游 `sched.proto` 接入 prost。
2. `hitrace.proto` 只补齐 sched message 字段。
3. sched 明细 Row 与 direct table builders 从 `sched.proto` 生成，所有 sched message 建成 SQL 明细表。
4. `process`、`thread`、`thread_state`、`instant`、`sched_slice`、`raw_event` 最小派生表可查询。
5. PR 说明包含 issue #25 checklist 项、issue #22 派生表关系、SQL 表变化、真实 trace 查询、workspace test 和 clippy 结果。
