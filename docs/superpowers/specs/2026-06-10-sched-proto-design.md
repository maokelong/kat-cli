# sched.proto 接入与 sched 表设计

## 背景

Issue [#25](https://github.com/maokelong/kat-rs/issues/25) 要求按 `types/plugins/ftrace_data` 清单逐步覆盖 htrace protobuf datasource。本次完成清单中的 `sched.proto`：接入上游 proto，解码 htrace 中的 sched ftrace events，并把 sched 事件暴露为可查询 SQL 表。

Issue [#22](https://github.com/maokelong/kat-rs/issues/22) 提到 `thread_state`、`instant` 这类表属于从事件流生成的派生表。issue #25 也说明这些表按 trace_streamer 语义属于 `ftrace_data` 派生表，可以跟随对应 sched 事件逐步补齐。

现有 kat-rs 已有最小 htrace 链路：读取 `ProfilerPluginData`，识别 `ftrace-plugin`，解码 `TracePluginResult.ftrace_cpu_detail.event.sched_switch_format`，并暴露 `sched_switch` 表。这个切片在保留现有 API 的基础上扩展为完整 sched 明细表，并先生成可验证的 `thread_state` / `instant` 派生表。

## 要解决的问题

1. 将上游 `sched.proto` 纳入 `kat-rs-datasource` 的 prost 生成流程。
2. 在 `hitrace.proto` 的 `FtraceEvent` 中补齐 sched oneof 分支，使 htrace 解码路径能识别 sched 事件。
3. 为 `sched.proto` 中每个 sched message 建表，表名使用 snake_case 事件名，例如 `sched_blocked_reason`、`sched_migrate_task`、`sched_switch`、`sched_wakeup`。
4. 每张 sched 明细表包含事件公共元数据和 proto message 字段，便于查询详细信息。
5. 从 sched 事件最小生成两张派生表：
   - `thread_state`：从 `sched_switch` 推导线程运行/等待状态区间。
   - `instant`：从 `sched_wakeup`、`sched_wakeup_new`、`sched_waking` 推导唤醒瞬时事件。
6. 用单元/端到端测试和真实 trace 查询证明新增表可查。

## 不做什么

1. 不接入 `types/plugins/ftrace_data/default`。
2. 不一次性接入 `ftrace_event.proto` 的全部非 sched oneof 分支。
3. 不复刻 trace_streamer 的完整进程/线程字典、arg_set、binder runnable、sched_slice 等语义。
4. 不引入 YAML lifecycle 配置引擎；issue #22 的通用 YAML 方案留给后续独立切片。
5. 不提交真实 trace fixture，也不把本地 `D:\项目\data\...htrace` 加入仓库。

## 上游 proto 来源

本次使用：

```text
D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\sched.proto
D:\项目\trace_streamer\src\protos\types\plugins\ftrace_data\ftrace_event.proto
```

`sched.proto` 原样复制到：

```text
crates/kat-rs-datasource/proto/ftrace_data/sched.proto
```

`ftrace_event.proto` 不整体复制；只把 sched oneof 分支和 tag 写入 kat-rs 的 `hitrace.proto`，避免把非 sched 的 300+ 分支带入本次 PR。

## 明细表契约

所有 sched 明细表都包含公共列：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `timestamp` | `uint64` | `FtraceEvent.timestamp` |
| `cpu` | `uint32` | 所属 `FtraceCpuDetailMsg.cpu` |
| `tgid` | `int32` | `FtraceEvent.tgid` |
| `comm` | `string` | `FtraceEvent.comm` |

每张表再追加对应 proto message 的字段：

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

当公共列与 message 字段同名时保留 message 字段名，并给公共列加清晰前缀：

```text
event_timestamp
event_cpu
event_tgid
event_comm
```

这样 `sched_wake_idle_without_ipi.cpu`、`sched_move_numa.tgid`、`SchedKthreadStopFormat.comm` 等字段不会与事件公共元数据冲突。

## 派生表契约

`thread_state` 是从 `sched_switch` 生成的最小区间表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `ts` | `uint64` | 状态开始时间 |
| `dur` | `uint64 or null` | 到下一次状态变化的持续时间 |
| `cpu` | `uint32 or null` | 运行态所在 CPU；非运行态为空 |
| `tid` | `int32` | 线程 id |
| `state` | `string` | `Running` 或 `prev_state:<value>` |
| `comm` | `string` | 线程名 |

生成规则：

1. 每个 `sched_switch` 产生一行 `next_pid` 的 `Running` 状态。
2. 每个 `sched_switch` 产生一行 `prev_pid` 的 `prev_state:<prev_state>` 状态。
3. 同一 `tid` 上一行状态在新状态开始时补齐 `dur`。
4. 最后一段没有结束事件时 `dur = null`。

`instant` 是从唤醒事件生成的最小瞬时表：

| 列 | 类型 | 说明 |
| --- | --- | --- |
| `ts` | `uint64` | 事件时间 |
| `name` | `string` | `sched_wakeup`、`sched_wakeup_new` 或 `sched_waking` |
| `ref` | `int32` | 被唤醒 tid，即 message `pid` |
| `wakeup_from` | `int32` | 触发唤醒的 event tgid |
| `ref_type` | `string` | 固定为 `tid` |
| `value` | `double` | 当前固定为 `0.0` |

这两个派生表先覆盖 trace_streamer 中 sched 相关的最小语义，不实现完整线程字典和 arg_set。

## 数据流

```text
ProfilerPluginData.data
  -> TracePluginResult
  -> FtraceCpuDetailMsg(cpu)
  -> FtraceEvent(timestamp, tgid, comm, oneof sched event)
  -> sched_* 明细表 rows
  -> thread_state / instant derived rows
  -> DataFusion MemTable
```

## 测试

1. `proto_contract` 验证上游 `SchedBlockedReasonFormat` 与 `SchedSwitchFormat` 能生成并 round-trip。
2. datasource 测试构造最小 `.htrace`，覆盖 `sched_blocked_reason`、`sched_migrate_task`、`sched_switch`、`sched_wakeup`、`sched_wakeup_new`、`sched_waking` 等表可查询。
3. datasource 测试验证未出现的 sched 表也能注册并返回 `count = 0`。
4. datasource 测试验证 `thread_state` 和 `instant` 从 sched 事件生成。
5. CLI 测试至少验证 `sched_switch` 和新增 sched 明细表查询 JSON。
6. 全量运行 `cargo test --workspace` 和 `cargo clippy --workspace --all-targets -- -D warnings`。
7. 对真实 trace 执行 `sched_switch`、`sched_wakeup`、`thread_state`、`instant` 的 count 查询。

## 最小交付

1. 上游 `sched.proto` 接入 prost。
2. `hitrace.proto` 只补齐 sched oneof 分支。
3. 所有 sched message 建成 SQL 明细表。
4. `thread_state` 和 `instant` 最小派生表可查询。
5. 在新分支提交并创建 PR，PR 说明包含 issue #25 checklist 项、issue #22 派生表关系、SQL 表变化、端到端测试、真实 trace 查询、workspace test 和 clippy 结果。
