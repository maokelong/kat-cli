# profiler upstream proto parity 与 special payload 契约确认设计

## 背景

Issue #25 已调整为按 upstream `developtools_profiler/protos` 逐步覆盖 profiler plugin payload。第一波工作包含两个子项：

- #47：对齐 profiler envelope 与当前已接入 plugin 的 upstream proto。
- #52：确认 special payload plugins 的 data 契约。

当前代码已经有 `.htrace -> profiler envelope -> domain decoder -> Arrow -> SQL` 链路，也已经接入 `ftrace_data/sched` 和 `native_hook`。但这几处仍有本地临时或裁剪 proto：

- `ProfilerPluginData` 仍在本地 `proto/hitrace.proto` 中定义。
- `native_hook_config.proto` 缺 upstream 字段 `dump_nmd`、`target_so_name`、`restrace_tag`。
- `ftrace_data` 当前只覆盖 sched direct tables，且 `TracePluginResult`、`FtraceEvent.common_fields` 和部分 sched 字段未与 upstream 对齐。
- `bytrace_plugin`、`hiperf_data`、`hiebpf_data` 只看到 config proto，data payload 不应在契约不清楚时强行当作普通 protobuf result message。

## 目标

1. 用项目内 `proto/profiler/profiler_plugin_data.proto` 承接 upstream `services/common_types.proto::ProfilerPluginData` 字段事实。
2. 补齐 `native_hook` config/result proto 与 upstream 的已知差异，保持现有 native hook direct/raw SQL 不回退。
3. 对齐当前已接入的 `ftrace_data/sched` schema 差异，保持现有 sched direct tables 不回退。
4. 确认 `bytrace_plugin`、`hiperf_data`、`hiebpf_data` 的 data payload 契约，并把结论写回 issue。
5. 保持 `.htrace` file reader、profiler envelope 机制层、domain decode、Arrow sink 的分层边界。

## 非目标

1. 不接入 `IProfilerService`、`IPluginService`、session control、online `FetchData` 或设备连接能力。
2. 不扩展 `ftrace_data` 非 sched family。
3. 不把 ftrace 改成完整 upstream oneof event 模型。
4. 不实现 TraceStreamer derived tables。
5. 不在 special payload 契约确认前新增 bytrace/hiperf/hiebpf decoder。
6. 不引入 payload manifest 或全局 schema framework。

## 设计方案

### 模块 A：Profiler envelope upstream 事实源

新增项目内 `proto/profiler/profiler_plugin_data.proto`，以 upstream `services/common_types.proto::ProfilerPluginData` 为字段事实源，只接入当前离线 datasource 需要的 profiler plugin data envelope 类型。

- `ProfilerPluginData.clock_id` 应改为 upstream enum，而不是当前 `int32`。
- `ProfilerPluginData.data` 仍需 `serde_bytes`，保证 `profiler_plugin_data` raw table 查询二进制列行为不变。
- 文件路径表达项目内 profiler envelope 语义，注释保留 upstream 来源，字段/tag 对齐 upstream，同时不引入 service RPC 能力。
- 现有 `formats/hitrace/profiler` 继续只处理 profiler envelope framing、dispatch 和 raw record，不理解具体 plugin payload。

### 模块 B：native_hook upstream parity

补齐当前 native_hook proto 与 upstream 的已知差异，让现有 descriptor-driven record/table 生成自然覆盖 direct/raw 查询面。

- `native_hook_result.proto` 目前与 upstream message/tag 基本对齐，保留项目 package 和当前生成路径即可。
- `native_hook_config.proto` 明确少字段 30-32，补齐后 `native_hook_config` direct table 应自然新增列。
- domain 仍只负责 `NativeHookConfig` / `BatchNativeHookData` decode 和 oneof 到 `NativeHookRecord`。
- Arrow sink 仍只生成 direct/raw tables，不做 alloc/free 配对、符号化或调用栈还原。

### 模块 C：ftrace_data sched parity

只补当前已接入 sched 链路的 upstream 字段差异：`TracePluginResult` stats/symbols/clock/version、`FtraceEvent.common_fields`、sched message 字段差异。

- `FtraceEvent` 可以新增 upstream `CommonFileds common_fields = 50`，但本轮不把 sched 字段移动进 oneof。
- `TracePluginResult` 补齐 upstream 字段，但 domain decode 仍只遍历 `ftrace_cpu_detail.event` 产出 direct sched records。
- 新增字段可被 protobuf decode 保留在 Rust 类型中，不意味着新增 SQL 表或 derived 语义。
- 非 sched family 和完整 ftrace oneof 模型留给后续 #51。

### 模块 D：special payload 契约确认

从 upstream proto、plugin 源码和可用真实 trace 样本确认 `bytrace_plugin`、`hiperf_data`、`hiebpf_data` 的 data payload 契约，再把结论写回 #52。

- 本轮只形成契约结论和后续建议。
- 如果发现 payload 是文本或特殊二进制，只记录 raw table / parser 边界，不新增 decoder。
- 如果发现存在 protobuf result message，再更新 #25/#52 或新建后续实现 issue。

### 模块 E：验证与回写

用 contract tests、datasource query tests、真实 trace 查询和 issue 回写保护本轮交付。

- proto parity 很容易出现字段/tag 回退，contract tests 必须覆盖。
- datasource query tests 保护现有 `profiler_plugin_data`、`sched_*`、`native_hook_*` 表不回退。
- #52 的价值在契约结论，必须写回 issue，否则完成状态不可 review。

## 预期改动面

代码和 proto：

```text
crates/kat-rs-datasource/proto/profiler/profiler_plugin_data.proto
crates/kat-rs-datasource/proto/hitrace.proto
crates/kat-rs-datasource/proto/native_hook/native_hook_config.proto
crates/kat-rs-datasource/proto/native_hook/native_hook_result.proto
crates/kat-rs-datasource/proto/ftrace_data/trace_plugin_result.proto
crates/kat-rs-datasource/proto/ftrace_data/ftrace_event.proto
crates/kat-rs-datasource/proto/ftrace_data/sched.proto
crates/kat-rs-datasource/build.rs
crates/kat-rs-datasource/tests/proto_contract.rs
crates/kat-rs-datasource/tests/hitrace_datasource_query.rs
crates/kat-rs-datasource/tests/hitrace_architecture_contract.rs
```

文档和 issue：

```text
docs/superpowers/specs/2026-06-18-profiler-upstream-proto-parity-and-special-payload-contract-design.md
GitHub issue #47
GitHub issue #52
```

不应修改：

```text
crates/kat-rs-datasource/src/formats/hitrace/file.rs
crates/kat-rs-datasource/src/query.rs
crates/kat-rs-datasource/src/catalog.rs
```

## 验收标准

1. `ProfilerPluginData` 来自 `proto/profiler/profiler_plugin_data.proto`，并与 upstream 字段/tag 对齐。
2. `profiler_plugin_data` raw table 仍可查询 `name`、`data`、`clock_id`、`version`、`sample_interval`。
3. `native_hook_config` 表包含 upstream 缺失字段 30-32 对应列。
4. `native_hook` direct/raw 查询测试继续通过。
5. `TracePluginResult`、`FtraceEvent.common_fields` 和 sched 字段差异完成 parity。
6. `sched_*` direct table 查询测试继续通过。
7. `bytrace_plugin`、`hiperf_data`、`hiebpf_data` 的 payload 契约结论写回 #52。
8. PR body 写明 #47/#52 的完成项、非目标和验证证据。

## 验证命令

```powershell
cargo fmt --all -- --check
cargo test -p kat-rs-datasource --test proto_contract -- --nocapture
cargo test -p kat-rs-datasource --test hitrace_datasource_query -- --nocapture
cargo test -p kat-rs-datasource --test hitrace_architecture_contract -- --nocapture
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

如有可用真实 trace 样本，继续执行：

```powershell
cargo run --release -p kat-rs-cli -- query <trace.htrace> "<SQL>"
```
