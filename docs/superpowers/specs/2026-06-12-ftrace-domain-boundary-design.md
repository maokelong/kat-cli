# ftrace datasource 架构边界收敛设计

## 背景

Issue [#27](https://github.com/maokelong/kat-rs/issues/27) 是 #25 / PR #26 之后的架构边界切片。PR #26 已验证 `.htrace -> ftrace-plugin -> sched direct tables -> DataFusion` 的纵向链路，但 review 指出当前实现容易把 `.htrace` 文件容器、profiler plugin envelope、ftrace 领域语义、Arrow 落表和 SQL 注册都固化到同一条过程式主链中。

当前 PR 原本只计划拆出 ftrace domain。由于 reviewer 希望这个 PR 直接完成更明确的架构边界，本设计把范围升级为 PR #26 review comment 里的第一阶段落地：建立 `formats/hitrace`、`domains/ftrace`、`sinks/arrow` 和 `catalog` 的代码边界，并用中立 `TraceRecord` / `TraceRecordSink` 断开 parser 直接写 Arrow 表的耦合。

## 事实源与层次

PR #26 review 给出的事实层次是：

```text
.htrace file
  -> TraceFileHeader
  -> length-prefixed ProfilerPluginData
      -> name = ftrace-plugin
          -> data = TracePluginResult
              -> FtraceCpuDetailMsg
                  -> FtraceEvent
                      -> sched_switch / sched_wakeup / ...
```

OpenHarmony `developtools_profiler` 也支持这个划分：

- `.htrace` 外层是 profiler trace file container，包含 `TraceFileHeader`、section length、`dataType` 等容器字段。
- `ProfilerPluginData` 是 profiler plugin envelope，`name` 决定 payload 属于哪个 plugin。
- `TracePluginResult`、`FtraceCpuDetailMsg`、`FtraceEvent` 属于 ftrace-plugin payload 语义。
- sched event 只是 ftrace event family 的第一批 direct tables，不应把 datasource 架构设计成“只有 sched 的 hitrace parser”。

因此本 PR 的职责边界调整为：

```text
formats/hitrace
  -> 读取 .htrace/profiler section
  -> streaming decode length-prefixed ProfilerPluginData
  -> 按 plugin name 分发 payload
  -> 只向 TraceRecordSink 推送中立 record

domains/ftrace
  -> decode TracePluginResult
  -> 遍历 FtraceCpuDetailMsg / FtraceEvent
  -> 产出 FtraceEventRecord
  -> 不写 Arrow，不注册 SQL 表

sinks/arrow
  -> 接收 TraceRecord
  -> 写 profiler_plugin_data raw table
  -> 写 sched direct event tables
  -> 输出 TraceDataset

catalog/query
  -> TraceDataset / TraceTable 描述可注册表
  -> query 只消费 dataset 并注册 DataFusion MemTable
```

## 目标

1. 新增 `formats/hitrace`，让 `.htrace` 容器解析不再作为 datasource 中心文件存在。
2. 新增 `domains/ftrace`，由 ftrace domain 独立负责 `TracePluginResult` 和 `FtraceEvent` 语义。
3. 新增 `catalog`，提供 `TraceRecord`、`TraceRecordSink`、`TraceDataset`、`TraceTable`、`TableCategory`。
4. 新增 `sinks/arrow`，把 `ProfilerPluginData` 和 sched direct event records 转换为 Arrow `RecordBatch`。
5. `query` 层只消费 `TraceDataset`，不直接依赖 hitrace/ftrace 内部 table builder。
6. 保持现有 SQL 表名、字段名、CLI 查询入口和 sched direct table 查询结果不变。

## 非目标

1. 不在本 PR 引入 perfetto、bytrace text、hiperf 等新输入格式。
2. 不补全 upstream ftrace 全量 schema、`common_fields`、oneof/descriptor 语义。
3. 不接入 sched 之外的 irq、binder、power 等新 event family。
4. 不实现 `thread_state`、`instant`、`sched_slice`、`process`、`thread`、`raw_event` 等派生表。
5. 不把 build script 的 proto 文本扫描升级为 descriptor-driven generator。
6. 不设计多 sink 插件系统；本 PR 只保留一个 Arrow sink，但 parser 与 sink 通过 trait 解耦。

## 设计

`catalog` 是解码链路的中立接口：

```rust
enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    FtraceEvent(FtraceEventRecord),
}

trait TraceRecordSink {
    fn push(&mut self, record: TraceRecord) -> Result<()>;
}
```

`formats/hitrace` 只负责 `.htrace` container 和 `ProfilerPluginData` envelope。它根据 `ProfilerPluginData.name == "ftrace-plugin"` 调用 ftrace domain decoder，但不理解 `TracePluginResult` 内部结构，也不创建 Arrow builder。

`domains/ftrace` 负责把 ftrace-plugin payload 解码为 `FtraceEventRecord`。该 record 包含 `EventContext` 和原始 `FtraceEvent`，让后续 sink 可以继续生成当前 sched direct tables。由于当前 proto 仍是本地裁剪版，本 PR 不补 `common_fields` 和完整 oneof，只保留后续 schema PR 的边界位置。

`sinks/arrow` 实现 `TraceRecordSink`。它接收 `ProfilerPluginData` 写入 `profiler_plugin_data` raw table，接收 `FtraceEventRecord` 后交给生成的 `SchedDirectTableBuilders` 写入 sched direct event tables。生成代码依赖 `domains::ftrace::FtraceEventRecord` 和 `sinks::arrow::{DirectEventTableBuilder, EventMeta}`，不再依赖 hitrace 或旧 `ftrace` 顶层模块。

`query` 创建 `ArrowSink`，调用 `formats::hitrace::decode_file`，再把 `TraceDataset` 中的 `TraceTable` 注册为 DataFusion `MemTable`。这样 query 只消费 catalog，而不关心输入格式和领域解码细节。

## 测试与验证

新增/更新架构契约测试：

- `formats/hitrace` 不包含 `TracePluginResult::decode`、`SchedDirectTableBuilders`、`ArrayBuilder`、`RecordBatch`。
- `domains/ftrace` 包含 `TracePluginResult::decode` 和 `TraceRecord::FtraceEvent`，但不包含 Arrow/table builder。
- `sinks/arrow` 实现 `TraceRecordSink`，并拥有 `SchedDirectTableBuilders` 和 direct table builder。
- `query` 消费 `TraceDataset`，不再消费 `load_hitrace_tables` 或旧 `FtraceTables`。
- 生成的 sched builders 接收 `FtraceEventRecord`，并依赖 `sinks::arrow`。

保留行为测试：

- `profiler_plugin_data` 仍可查询。
- sched direct tables 仍可查询，字段名和表名不变。
- malformed/unsupported hitrace section 行为保持可验证。
- scalar SQL 查询仍可在 datasource 上执行。

完整验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
