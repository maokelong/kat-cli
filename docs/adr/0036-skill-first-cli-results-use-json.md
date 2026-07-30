---
status: accepted
---

# KAT Response 使用 JSON tagged union

KAT Skill 是第一公民；外层参数一旦形成具体操作，CLI 无论成功失败都只向 stdout 写一个短命、compact 的 KAT Response，显式 help 仍是普通文本，Clap parse failure 只写 stderr，而 Response 不透传 Runtime、充当持久状态或建立独立兼容面。

KAT Response 是由 CLI 从 typed facts 组装的封闭 `status` tagged union：success 只有 operation-specific `result`，failure 只有 ADR 0037 的 Diagnostic，已成功交付的日志或测试报告才作为显式顶层证据；不使用无类型 JSON、部分结果、占位 null、通用 metadata bag 或内部标识，operation-specific 内容只保留 Skill 完成任务所需的产品事实。

全部强制门和承诺的持久交付物成功后才能发布 success；publisher 输出单行 JSON 加 LF，业务成功/help 为退出码 0，操作或发布失败为 1，解析失败为 2，stdout/stderr I/O 失败不生成备用 Response，也不回滚此前已发布的 Run、Dataset 或报告。
