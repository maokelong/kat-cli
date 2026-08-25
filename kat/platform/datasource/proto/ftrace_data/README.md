# ftrace protobuf 快照

本目录是 `kat.hitrace.TracePluginResult` 与 `kat.hitrace.TracePluginConfig` 的固定编译闭包，不是运行时可替换的用户 schema。Issue #213 同时激活 `ftrace-plugin/Data` 与 `ftrace-plugin_config/Config`：两条 route 在旧 decoder 前由 `NativeHookSourceCapture` 认领，只执行 typed decode 与 generated emitter/capture，不发布旧规范化表，也不执行 materializer 的 ftrace 准入。materializer 保留 PR #216 的本地校验实现，供仍进入 legacy decoder 的路径使用；新 Source routes 的完整性要求由独立合同测试验证。

## 上游基线

- 仓库：OpenHarmony `developtools_profiler`
- revision：`73d26bb5acfcafb2b1f4f94ead5640241d1e5f73`
- 目录：`protos/types/plugins/ftrace_data/default`
- 根文件：`trace_plugin_result.proto`、`trace_plugin_config.proto`
- `ftrace_event.proto` blob：`c602570eb226982fb41537b6bc32c21fa2a9f60c`
- `trace_plugin_result.proto` blob：`470e6b76dd917163968b829ccfd3224a66799ffa`
- `trace_plugin_config.proto` blob：`d1783463f51209b6d03918e98160686209b13b35`
- 生成器：同 revision 的 `device/plugins/ftrace_plugin/tools/ftrace_proto_generator.py`，blob `bc1006d210b70f640e96fb26e67e1963d1b02160`

上游目录在该 revision 下包含 39 个 `.proto` 文件；本目录也固定包含相同的 39 个文件。`ftrace_event.proto` 的完整 event oneof 和两个 root 文件以该 revision 为准。

## KAT 适配

Vendor 后只允许以下仓库内适配：

1. 所有消息统一进入 `package kat.hitrace`，避免把上游包名直接变成 KAT 的公共 Rust 模块边界。
2. `ftrace_event.proto` 与 `trace_plugin_result.proto` 的 import 增加 `ftrace_data/` 前缀，以匹配 `build.rs` 的 proto include root。
3. `sched.proto` 保留 KAT 已有 legacy ftrace decoder 使用的字段合同。它最初在 KAT commit `a24720dcf2db86dbe202d1f2e0809025415157ef` 引入，当前文件顶部明确标记为 Trace Streamer adaptation；#213 不借 schema vendor 改写旧 decoder 合同。

第 3 项是有意的 compatibility overlay：descriptor-derived ftrace tables 与旧手写表不承诺兼容，route activation 也不能顺手改变仍保留的 legacy decoder。升级者必须分别核对 upstream default `sched.proto`、KAT 当前 `sched.proto` 和真实 trace wire differential，不能静默用上游文件覆盖。

## 升级步骤

1. 选择并记录新的 `developtools_profiler` commit；从上述 upstream 目录取得完整 39-file closure，同时取得同 revision 的生成器。
2. 对除 `sched.proto` 外的文件只应用 package/import 两类机械改写。若出现其他语义 diff，先在独立 issue/ADR 说明原因。
3. 对 `sched.proto` 做字段号、wire type、message name 的三方比较；需要改变 KAT compatibility overlay 时，必须同时验证 legacy decoder 和 descriptor-derived tables。
4. 更新本文件中的 revision、root blob 与 generator blob；确认本目录仍恰好包含 39 个 `.proto` 文件。
5. 运行 `proto_contract`、profiler source capture synthetic differential 和真实 ftrace independent wire census；最后运行 workspace test、clippy、check、fmt 与 `git diff --check`。

快照升级不得只替换 `ftrace_event.proto`：两个 roots、全部 imports、生成器来源和 compatibility overlay 必须作为一个可 review 的变更提交。
