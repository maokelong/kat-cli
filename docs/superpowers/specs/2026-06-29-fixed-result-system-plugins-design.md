# 系统资源 fixed result plugins 覆盖设计

## 背景

Issue #53 是 #25 的子项，目标是覆盖一批系统资源类 profiler plugins：

- `cpu_data`
- `memory_data`
- `process_data`
- `diskio_data`
- `network_data`
- `gpu_data`

这批 plugin 与 `ftrace`、`native_hook` 的 payload shape 不同。它们没有 ftrace 的 CPU/event family，也没有 native_hook 的 batch + oneof event，而是普通的 config/result protobuf payload：

```text
ProfilerPluginData
  -> <Plugin>Config  // *_config envelope
  -> <Plugin>Data    // data envelope
```

因此本轮不应该把它们接入 ftrace 或 native_hook 路径，也不应该借机引入 payload manifest 或通用 schema framework。合理切片是新增一个 `fixed_result` domain：它承接这批“单个 config message + 单个 root result message”的 profiler plugin，并将 decoded root payload 直接投影为 Arrow 表。

## Upstream 依据

upstream 来源为 `developtools_profiler` master `d73eace8fc90e92c5492cfaf0a645ce290ce829d`：

| plugin | runtime plugin name | upstream proto 路径 | config message | result root message |
| --- | --- | --- | --- | --- |
| cpu_data | `cpu-plugin` | `protos/types/plugins/cpu_data/cpu_plugin_config.proto`, `cpu_plugin_result.proto` | `CpuConfig` | `CpuData` |
| memory_data | `memory-plugin` | `protos/types/plugins/memory_data/memory_plugin_common.proto`, `memory_plugin_config.proto`, `memory_plugin_result.proto` | `MemoryConfig` | `MemoryData` |
| process_data | `process-plugin` | `protos/types/plugins/process_data/process_plugin_config.proto`, `process_plugin_result.proto` | `ProcessConfig` | `ProcessData` |
| diskio_data | `diskio-plugin` | `protos/types/plugins/diskio_data/diskio_plugin_config.proto`, `diskio_plugin_result.proto` | `DiskioConfig` | `DiskioData` |
| network_data | `network-plugin` | `protos/types/plugins/network_data/network_plugin_config.proto`, `network_plugin_result.proto` | `NetworkConfig` | `NetworkDatas` |
| gpu_data | `gpu-plugin` | `protos/types/plugins/gpu_data/gpu_plugin_config.proto`, `gpu_plugin_result.proto` | `GpuConfig` | `GpuData` |

runtime plugin name 来自 upstream `device/plugins/*_plugin/src/*_module.cpp` 的 `g_pluginModule.name`。proto 文件中的 message/field/tag 是 payload schema 事实；runtime name 只用于 profiler envelope dispatch。

## 目标

1. 将 6 个 plugin 的 upstream proto 迁入 `crates/kat-rs-datasource/proto/<plugin>/`，保留 message 名、字段名和 tag number。
2. 通过 `prost_build` 生成 Rust proto 类型，包含 config/result 以及 result 内部 nested message。
3. 新增 `domains/fixed_result`，decode 这批 plugin 的 config/data envelope，并产出 `FixedResultRecord`。
4. 在 `TraceRecord` 增加粗粒度 `FixedResult` variant，不把 6 个 plugin 的内部 message 摊平成全局 record。
5. 在 Arrow sink 新增 `FixedResultTableSet`，暴露每个 plugin 的 config/result direct tables。
6. 保持 `profiler_plugin_data` raw table 行为不变。
7. 增加 proto contract、domain/record contract 和 datasource query 测试。

## 非目标

1. 不实现 TraceStreamer derived tables。
2. 不做跨 plugin 归一化，例如统一 CPU/memory/process 时间轴。
3. 不拆解所有 repeated child message 为独立 child table。
4. 不引入 payload shape manifest、通用 descriptor schema 层或运行时反射 decoder。
5. 不修改 `.htrace` file reader 或 profiler envelope 机制层。
6. 不把 plugin name、config envelope name 当成 payload schema 真相。
7. 没有真实 trace 样本时，不伪造“真实 trace 查询结果”；用合成 `.htrace` 覆盖端到端查询，并在 PR/issue 中说明真实样本缺口。

## 设计方案

### 模块 A：proto 归属

新增 proto 路径：

```text
crates/kat-rs-datasource/proto/cpu_data/*
crates/kat-rs-datasource/proto/memory_data/*
crates/kat-rs-datasource/proto/process_data/*
crates/kat-rs-datasource/proto/diskio_data/*
crates/kat-rs-datasource/proto/network_data/*
crates/kat-rs-datasource/proto/gpu_data/*
```

每个目录对应 upstream `protos/types/plugins/<plugin>/`。项目内 proto 增加稳定 package，例如 `kat.cpu_data`、`kat.memory_data`。`memory_data` 的 import 改为项目内 include root 下的 `memory_data/memory_plugin_common.proto`，只调整路径，不改字段语义。

### 模块 B：fixed_result domain

新增 `domains/fixed_result`：

```text
ProfilerPluginData envelope
  -> PluginPayloadRegistry
  -> FixedResultPluginDecoder
  -> decode <Plugin>Config 或 <Plugin>Data
  -> FixedResultRecord
  -> TraceRecord::FixedResult
```

`FixedResultRecord` 是 domain 内部 enum，包含 12 个 variant：6 个 config + 6 个 result。这个 enum 由 build helper 根据静态 `FixedResultPluginSpec` 生成，避免重复手写 decode 和 record variant。

### 模块 C：Arrow direct tables

新增 generated `FixedResultTableSet`，每个 config/result root message 一个 direct table：

```text
cpu_config, cpu_data
memory_config, memory_data
process_config, process_data
diskio_config, diskio_data
network_config, network_data
gpu_config, gpu_data
```

这些表使用现有 `MessageTableBuilder<T>`，保留 protobuf nested/repeated 字段为 Arrow nested/list 字段。本轮不拆 child table，因为 issue 的这批 plugin 是 fixed root result，拆所有 repeated child 会把本轮推回已放弃的通用 schema/table framework。

### 模块 D：build helpers

新增两个 build helper：

| 文件 | 消费者 | 职责 |
| --- | --- | --- |
| `build/fixed_result_domain_codegen.rs` | `domains/fixed_result` | 静态 plugin spec、proto 文件列表、serde derive message 路径、生成 `FixedResultRecord` 和 decoder specs |
| `build/fixed_result_arrow_codegen.rs` | `sinks/arrow` | 生成 `FixedResultTableSet` 和 table builder routing |

descriptor 只用于收集 message/nested message 事实，帮助 `serde` derive 覆盖 nested payload。它不决定 runtime plugin name，也不决定哪些 plugin 属于本批次；本批次列表来自 issue #53。

### 模块 E：验证

测试分三层：

1. `proto_contract`：确认 6 个 plugin 的 Rust proto 类型可 encode/decode，`TraceRecord::FixedResult` 保持粗粒度。
2. `hitrace_architecture_contract`：确认 fixed_result 没有污染 `.htrace` file reader/profiler 机制层，build helper 按消费者拆分。
3. `hitrace_datasource_query`：用合成 `.htrace` 同时写入 6 个 plugin 的 config/data envelope，查询 12 张 direct tables 中的代表性字段，并确认 raw `profiler_plugin_data` 仍可查。

## 头脑风暴校验

- 方案“每个 plugin 手写一套 decoder/table”：最直接，但会在 6 个 plugin 上重复同一模式，后续 fixed result plugin 继续扩散。
- 方案“运行时 prost-reflect + descriptor 自动投影”：改动面太大，会引入新的 schema/runtime 反射层，偏离当前架构初心。
- 选择方案“静态 FixedResultPluginSpec + build-time 机械生成”：plugin 列表仍由 domain 语义确认，descriptor 只提供 proto 事实；能减少重复，又不把 ftrace/native_hook 强行纳入同一模型。

## 验收标准

1. Issue #53 范围内每个 plugin 都记录 upstream proto 路径。
2. 6 个 plugin 的 proto 均进入 `prost_build`，并生成可 encode/decode 的 Rust 类型。
3. 每个 plugin 的 payload shape 和 root result message 在 spec 与代码中明确。
4. 每个 plugin 的 config/data envelope 能通过 domain decode 产出 `FixedResultRecord`。
5. Arrow sink 暴露 12 张 fixed result direct tables。
6. 合成 `.htrace` 端到端查询能查到每个 plugin 的代表性字段。
7. `.htrace` file reader、profiler envelope 机制层、ftrace、native_hook 行为不回退。

## 验证命令

```powershell
cargo fmt --all -- --check
cargo test -p kat-rs-datasource --test proto_contract -- --nocapture
cargo test -p kat-rs-datasource --test hitrace_architecture_contract -- --nocapture
cargo test -p kat-rs-datasource --test hitrace_datasource_query -- --nocapture
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
