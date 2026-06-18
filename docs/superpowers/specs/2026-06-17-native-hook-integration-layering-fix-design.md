# native_hook 接入与 profiler 分层修复设计

## 背景

`2026-06-12-ftrace-domain-boundary-design.md` 建立了第一条 `.htrace -> ftrace-plugin -> domain -> Arrow -> query` 链路的边界。它解决了 parser 直接写 Arrow 表、query 层感知 ftrace 细节、`.htrace` 容器与 ftrace payload 混杂等问题。

`native_hook` 是这套边界后的第一个新 profiler plugin。它不是 ftrace event family，也不应该塞进 ftrace 的表生成路径。它的 payload 形态是：

```text
ProfilerPluginData
  -> NativeHookConfig
  -> BatchNativeHookData
      -> repeated NativeHookData
          -> NativeHookData.event oneof
```

这条链路暴露的问题不是“需要一个新的全局 payload schema 架构”，而是上一版分层中仍有几处 ftrace 假设没有收住：

- profiler envelope 机制曾以顶层 `plugin_flow` 出现，容易被误读为全项目 plugin framework。
- length-prefixed protobuf message 的命名过于底层，真实语义是 profiler envelope framing。
- `record` 和 `sinks/arrow` 已经能承接第二个 domain，但不能继续把每个 plugin 的内部事件都摊平成全局模型。
- `build.rs` 里混有 prost 编译、serde derive、ftrace table 生成、native hook record/table 生成，需要按消费者拆清楚。
- Arrow table builder 中有公共能力，也有 ftrace/native_hook 专用投影逻辑，不能因为结构相似就合并成含混的公共语义。

## 目标

1. 新增 `native_hook` proto 归属、prost 编译和必要 serde derive。
2. 新增 `domains/native_hook`，负责 `NativeHookConfig` 和 `BatchNativeHookData` 的 payload decode。
3. 在 native hook domain 内把 `NativeHookData.event` oneof 转换成 `NativeHookRecord`。
4. 让 Arrow sink 消费 `NativeHookRecord` 并生成 native hook direct/raw tables。
5. 保持 `.htrace` file reader 不感知 ftrace/native_hook payload 语义。
6. 将 profiler envelope/framing/registry 机制明确收进 `formats/hitrace/profiler`。
7. 保持 `TraceRecord` 为粗粒度跨层传输边界，不合并 `Ftrace` 和 `NativeHook`，也不展开所有 plugin 内部事件。
8. 拆清 `sinks/arrow` 中通用 table builder 与 domain-specific table projection。
9. 拆清 `build.rs` 职责：prost/proto 编译、ftrace Arrow codegen、native hook domain codegen、native hook Arrow codegen 分别服务不同消费者。
10. 保持既有 ftrace sched direct table、`profiler_plugin_data` raw table 和 query 行为不变。

## 非目标

1. 不实现 TraceStreamer 的 `native_hook`、`native_hook_frame`、`native_hook_statistic` 等 derived tables。
2. 不实现 alloc/free、mmap/munmap 生命周期配对、符号化、栈还原或跨事件归一化。
3. 不引入 payload shape manifest。
4. 不引入跨 plugin 的通用 descriptor schema 层。
5. 不把 ftrace 和 native_hook 强行合并成同一种 payload 模型。
6. 不把 plugin name、alias 或 config envelope name 当作 payload schema 真相。
7. 不把 native_hook 接入 `FTRACE_EVENT_FAMILIES`。
8. 不重写 profiler plugin 分发机制为通用插件系统。
9. 不设计多 sink 插件系统。

## 分层设计

### `formats/hitrace/file`

职责是读取 `.htrace` container：header、section offset、section length、data type 和 section body。

边界要求：

- 不识别 `ftrace-plugin`、`nativehook`、`hookdaemon` 等 plugin name。
- 不 decode `TracePluginResult`、`NativeHookConfig` 或 `BatchNativeHookData`。
- 不创建 Arrow builder，不注册 SQL 表。

### `formats/hitrace/profiler`

职责是处理 `.htrace` profiler section 内部的 profiler envelope 机制：

- profiler envelope framing：读取 length-prefixed `ProfilerPluginData`。
- envelope：建模 plugin name、config/data envelope、section offset、sample interval 等外壳信息。
- registry：提供 `PluginDecoder`、`PluginDecoderSpec` 和 dispatch 机制。
- payload error context：把 section offset、version、sample interval 等 profiler 上下文带入错误信息。

边界要求：

- 这是 `.htrace` profiler 子格式层，不是全项目通用 plugin framework。
- registry 只调度外部传入的 decoder specs，不内置具体 domain decoder。
- 它不理解 `NativeHookData.event`、`FtraceEvent` 或 Arrow table。

装配层可以知道具体启用哪些 domain decoder。机制层不能反向拥有 domain 列表。

### `domains/ftrace`

职责是 ftrace payload 语义：

- decode `TracePluginResult`。
- 遍历 `FtraceCpuDetailMsg` 和 `FtraceEvent`。
- 产出 `FtraceRecord` / `FtraceEventRecord`。

边界要求：

- 不写 Arrow。
- 不注册 SQL。
- 不承担 native hook 或其他 profiler plugin 的 schema。

ftrace 不是 native hook 的父模型，也不是所有 profiler plugin 的通用模型。

### `domains/native_hook`

职责是 native hook payload 语义：

- decode `nativehook_config` envelope 为 `NativeHookConfig`。
- decode native hook data envelope 为 `BatchNativeHookData`。
- 遍历 `BatchNativeHookData.events`。
- 将 `NativeHookData.event` oneof 转换成 `NativeHookRecord`。
- 明确当前支持哪些 direct/raw record，哪些复杂语义暂不承诺。

边界要求：

- 可以依赖 native hook proto 类型。
- 可以拥有 oneof 到 domain record 的转换逻辑。
- 不创建 Arrow array，不注册 SQL 表。

### `record`

职责是连接 domain decoder 和 sink 的粗粒度 record stream。

推荐形态：

```rust
enum TraceRecord {
    ProfilerPluginData(ProfilerPluginData),
    Ftrace(Box<FtraceRecord>),
    NativeHook(Box<NativeHookRecord>),
}
```

边界要求：

- `TraceRecord` 不展开 `Alloc`、`Free`、`SchedSwitch` 等 domain 内部事件。
- 不合并 `Ftrace` 和 `NativeHook`。两者 payload 模型不同，合并会把差异转移到更隐蔽的位置。
- 不承载 sink 生命周期或表物化控制信号。

### `sinks/arrow`

职责是把粗粒度 domain record 物化为 Arrow tables，并输出 `TraceDataset`。

内部边界：

| 文件/模块 | 职责 |
| --- | --- |
| `sinks/arrow/table` | 通用 `MessageTableBuilder`、`EventTableBuilder`、empty table、`TraceTable` 物化 |
| `sinks/arrow/ftrace` | ftrace record 到 ftrace direct tables 的投影辅助 |
| `sinks/arrow/native_hook` | native hook record 到 native hook direct/raw tables 的投影辅助 |
| `sinks/arrow/mod` | 持有各 table set，按 `TraceRecord` 粗粒度分发，输出 dataset |

边界要求：

- 通用 table builder 不包含 ftrace/native_hook 语义。
- ftrace/native_hook table helper 可以知道各自 domain record。
- sink 不直接 decode protobuf payload。
- sink 不直接 match native hook protobuf oneof；oneof 到 record 的转换在 domain 层完成。

### `build.rs` 与 build helpers

职责是构建期机械生成，不是业务策略层。

应按消费者拆分：

| 部分 | 消费者 | 职责 |
| --- | --- | --- |
| proto 编译 | runtime domain decode | 使用 `prost_build` 编译 proto，生成 Rust proto 类型 |
| proto descriptor 辅助 | build-time codegen | 从 descriptor 获取 message/oneof/field 事实，避免自写 proto 文本 parser |
| ftrace Arrow codegen | `sinks/arrow/ftrace` | 根据 ftrace event family 配置生成 ftrace table builders |
| native hook domain codegen | `domains/native_hook` | 根据 `NativeHookData.event` oneof 生成 `NativeHookRecord` 相关重复代码 |
| native hook Arrow codegen | `sinks/arrow/native_hook` | 根据 native hook event record 生成 table set 和 builders |

descriptor 只提供 schema 事实，例如 message、field、oneof。它不能决定 plugin 业务语义，也不能升级成新的通用 schema 层。

## native_hook 接入设计

### Proto 归属

新增 proto 位于：

```text
crates/kat-rs-datasource/proto/native_hook/native_hook_config.proto
crates/kat-rs-datasource/proto/native_hook/native_hook_result.proto
```

迁移规则：

- 保留 OpenHarmony developtools_profiler 中 native hook proto 的 message 名、字段名和 tag number。
- package 使用项目内稳定命名。
- 不合并 config 和 result proto。
- 不为了当前 Arrow 表方便而改 proto 字段语义。

### Decode 流程

```text
ProfilerPluginData
  -> formats/hitrace/profiler envelope
  -> domains/native_hook decoder
  -> NativeHookConfig 或 BatchNativeHookData
  -> NativeHookRecord
  -> TraceRecord::NativeHook
  -> sinks/arrow/native_hook
```

config envelope 解为 `NativeHookRecord::Config`。

data envelope 解为 `BatchNativeHookData`，遍历 `events` 后，将每个 `NativeHookData` 的公共上下文和 oneof payload 合并成对应 `NativeHookRecord`。

plugin name 是 `.htrace` profiler envelope 的运行时分发信息。它只用于把 envelope 送到正确 decoder，不是 native hook payload schema 的组成部分。

### Table 投影

当前只承诺 direct/raw 查询面：

- `native_hook_config`
- `native_hook_alloc`
- `native_hook_free`
- `native_hook_mmap`
- `native_hook_munmap`
- `native_hook_mem_tag`
- `native_hook_statistics`
- native hook map / symbol / stack / frame 类辅助 direct tables
- `native_hook_trace_alloc`
- `native_hook_trace_free`

这些表是 payload 直接投影加必要上下文字段，不承诺 TraceStreamer derived table 语义。

对于 frame、stack、symbol table、maps 这类结构，当前只能表达“可查询的 direct/map 数据”。它们不是完整调用栈重建、符号化或生命周期分析结果。

## 上一版架构问题修复

### 顶层 `plugin_flow` 命名与归属

问题：顶层 `plugin_flow` 容易被理解为全项目 plugin framework，但它实际只处理 `.htrace` profiler section 中的 `ProfilerPluginData` envelope。

修复：收进 `formats/hitrace/profiler`，让路径表达真实归属。

### `len_prefixed_message` 命名

问题：`len_prefixed_message` 描述了字节编码方式，但没有表达业务语义，容易让调用方忽略它属于 profiler envelope framing。

修复：命名为 profiler envelope framing。代码和错误信息都应围绕 `profiler envelope frame` 表达。

### `TraceRecord` 粒度

问题：新增 native hook 后，如果把每个 oneof event 都加到全局 `TraceRecord`，record 会变成所有 plugin 内部 schema 的中心枚举。

修复：`TraceRecord` 只保留 coarse domain variants。domain 内部事件由各自 domain record 表达。

### Arrow table builder 边界

问题：ftrace 和 native hook 的 table builder 长得相似，但相似的是 Arrow 物化技术，不是领域语义。

修复：公共能力只下沉到 `sinks/arrow/table`。ftrace/native_hook 的投影 helper 分开放，避免把 `EventMeta` 或 native hook context 伪装成通用 domain 模型。

### Event metadata 归属

问题：如果把 `EventMeta` 抽成公共 domain 模型，容易混淆 domain 和 sink 职责。

修复：domain record 保留自己的上下文；生成的 table builder 从 record/context 取物化需要的字段。`EventMeta` 只应是 Arrow 投影辅助，而不是跨 domain 语义模型。

### build.rs 职责

问题：新增 native hook 后，`build.rs` 容易继续膨胀，或者反过来抽象成过大的通用 generator。

修复：按消费者拆 helper。允许 ftrace/native_hook 生成器并存，但每个 helper 只服务明确消费者；不做跨 plugin 的大一统 schema 设计。

## 新增 plugin 后的预期改动面

新增一个 profiler plugin 仍然需要人工确认 domain 语义。合理的目标不是“只改 proto”，而是把改动限制在正确层级：

```text
proto/<plugin>/*
domains/<plugin>/*
build helper（如需要机械 codegen）
sinks/arrow/<plugin>.rs（如需要 Arrow 查询面）
record.rs（新增 coarse domain variant 时）
tests
```

不应该污染：

```text
formats/hitrace/file
formats/hitrace/profiler 机制层
catalog/query
已有 plugin domain
```

如果某个新 plugin 只是 `.htrace` profiler payload，`.htrace` file reader 不应因此变化。如果它不是 `.htrace` 输入格式，则应该新增或扩展 `formats/*`，而不是把输入格式逻辑塞进 domain。

## 错误处理与兼容性

- `.htrace` header、section 长度、data type 错误属于 format-level error。
- profiler envelope framing 解码失败属于 profiler mechanism error。
- 已知 plugin 的 payload decode 失败属于 domain payload error。
- 已知 plugin 的 config/data envelope 应带上 profiler section offset、version、sample interval 等上下文。
- 未注册 plugin 的 envelope 可以保留在 `profiler_plugin_data` raw table，但不进入 domain decoder。
- protobuf 未来字段如果 prost 当前类型不认识，本轮 direct table 不承诺可见；这不是 unknown event policy，而是静态 proto 类型的自然边界。

## 验收标准

1. 文档不再把 ftrace 描述成 native hook 风格的 protobuf oneof。
2. 文档不引入 payload shape manifest。
3. 文档不把 descriptor 描述成当前要落地的通用 schema 层。
4. 文档明确 domain 承接 payload 语义，sink 承接 Arrow 物化，build.rs 承接机械 codegen。
5. 文档明确当前 native hook 只交付 direct/raw 查询面，不承诺 TraceStreamer derived tables。

## 验证

文档变更的最小验证：

```powershell
git diff --check
```

如果同一提交还包含代码变更，则继续执行：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
