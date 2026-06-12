# ftrace 领域边界拆分设计

## 背景

Issue [#27](https://github.com/maokelong/kat-rs/issues/27) 是 #25 后续小 PR 的架构边界切片。#25 / PR #26 已验证 `.htrace -> ftrace-plugin -> sched direct tables -> DataFusion` 纵向链路，但 review 指出当前实现把 htrace/profiler 文件容器解析、ftrace-plugin payload 解码、sched direct table 写入放在同一层。

本次只做最小边界收敛：把 ftrace 领域逻辑从 `hitrace` 入口拆出。它不是完整 datasource 重构，也不引入 `formats/`、`domains/`、`sinks/`、catalog 或 `TraceRecord` 抽象。

## 事实源与边界判断

PR #26 的架构 review 给出的核心边界是：

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

OpenHarmony `developtools_profiler` 当前 commit `9c0250d0a2c93d153f50150756045fd3fcd1c22f` 也支持这个划分：

- `TraceFileHeader` 定义 `.htrace` 外层 profiler container，包含 `HEADER_SIZE = 1024`、`HEADER_MAGIC = 0x464F5250534F484F`、`dataType`、`segments`、`sha256` 等字段。
- `TraceFileReader::Read` 按 4 字节长度前缀读取 protobuf message，再 `ParseFromArray`。
- `ProfilerPluginData` 是 profiler envelope，字段包含 `name`、`status`、`data`、`clock_id`、`tv_sec`、`tv_nsec`、`version`、`sample_interval`。
- `TracePluginResult` 和 `FtraceEvent` 属于 ftrace 领域；上游 `FtraceEvent` 有公共字段和大 `oneof event`，但本次仍只保留当前 sched direct tables 所需的本地裁剪 schema。

因此，本次代码边界采用：

```text
hitrace entry
  -> 读取 .htrace/profiler section
  -> streaming decode ProfilerPluginData
  -> 保留 profiler_plugin_data 表写入
  -> 按 plugin name 分发 ftrace-plugin payload

ftrace module
  -> decode TracePluginResult
  -> 遍历 FtraceCpuDetailMsg / FtraceEvent
  -> 写入现有 sched direct table builders
  -> 返回现有 query 层可注册的 RecordBatch 表
```

## 目标

1. 新增 `ftrace` 领域模块，承接 ftrace-plugin payload 解码和 sched direct table 写入。
2. `hitrace` 入口不再直接引用 `TracePluginResult` 或 `SchedDirectTableBuilders`。
3. `hitrace` 入口保留 `.htrace` header/section、len-prefixed `ProfilerPluginData` streaming decode、`profiler_plugin_data` 表写入和 plugin 分发。
4. 保持现有 SQL 表名、字段名、CLI 查询入口和 sched direct table 查询结果不变。
5. 保持现有 build script 生成 sched direct table builders 的方式，不在本 PR 改 generator 架构。

## 非目标

1. 不做完整 `formats/hitrace` 目录拆分。
2. 不引入 `TraceRecord`、sink trait、多 sink 机制或 Arrow/DataFusion sink 解耦。
3. 不引入 `TraceDataset`、`TableCatalog`、`TableCategory` 或完整 datasource catalog。
4. 不补全 upstream ftrace 全量 schema、`common_fields` 或 oneof/descriptor 语义。
5. 不接入 sched 之外的新事件域。
6. 不实现 `thread_state`、`instant`、`sched_slice`、`process`、`thread`、`raw_event` 等派生表。
7. 不把 build script 的 proto 文本扫描升级为 descriptor-driven generator。

## 设计

新增 `crates/kat-rs-datasource/src/ftrace/mod.rs`，并把现有 direct event table 写入胶水从 `hitrace` 移到 `ftrace` 领域下。`ftrace` 模块暴露小接口：

```rust
pub(crate) const FTRACE_PLUGIN_NAME: &str = "ftrace-plugin";

pub(crate) struct FtraceTables { ... }

impl FtraceTables {
    pub(crate) fn new() -> Result<Self>;
    pub(crate) fn push_plugin_payload(&mut self, data: &[u8], section_start: usize) -> Result<()>;
    pub(crate) fn into_tables(self) -> Result<Vec<FtraceTable>>;
}
```

`hitrace` 入口只根据 `ProfilerPluginData.name` 判断是否分发给 `FtraceTables::push_plugin_payload`。错误上下文仍保留 profiler section byte offset，因为这是 htrace container 能提供的定位信息。

`FtraceTable` 只是本次切片需要的轻量返回结构，包含 `name` 和 `RecordBatch`。它不是 catalog，也不表达表类别、元数据或 datasource 统一模型。

`profiler_plugin_data` 表仍是 htrace/profiler envelope 表，通用 `TableBuilder<ProfilerPluginData>` 保留在 `hitrace` 下，不放进 ftrace 领域模块。

生成的 `SchedDirectTableBuilders` 应改为依赖 `crate::ftrace::{DirectEventTableBuilder, EventMeta, FtraceTable}`。这样 sched direct table 写入依赖 ftrace domain，而不再反向依赖 hitrace 入口。

## 测试与验证

新增架构契约测试：

- `hitrace.rs` 不再包含 `TracePluginResult`、`SchedDirectTableBuilders`、`decode_sched_message` 等 ftrace/sched 领域实现痕迹。
- `src/ftrace/mod.rs` 包含 `TracePluginResult::decode`、`SchedDirectTableBuilders::new()?` 和 payload 写入入口。
- 生成的 sched builders 使用 `crate::ftrace` 的 table builder 类型。

保留并运行现有行为测试：

- sched direct tables 查询结果保持不变。
- CLI 查询方式保持不变。
- `profiler_plugin_data` 仍可查询。
- malformed/unsupported hitrace section 行为保持不变。

完整验证命令：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
