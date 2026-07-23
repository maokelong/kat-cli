---
status: accepted
---

# inspect_pack Runtime failure 保留私有所有者

`inspect_pack` Runtime failure 的 Diagnostic 可以来自两条不同边界：Runtime 无法接受 CLI 生成的 request，或者 operation 已成立但 PACK Interface 形成失败。两者都使用同一 Diagnostic value schema，却不能在 CLI 内部被折叠成同一种业务失败；否则请求兼容性故障会被错误归为 PACK failure。仅靠 `message` 或 `help` 文本恢复所有者会把严格协议退化成字符串解析。

`inspect_pack` Runtime failure envelope 因此精确包含 `status: "failure"`、封闭的 `failure_owner` 与 `error`，其中 `failure_owner` 只接受 `"runtime_request" | "pack"`：request 解码或兼容性校验失败使用前者，PACK 扫描、导入和 Interface 形成失败使用后者。`failure_owner` 是 `inspect_pack` Runtime 与 CLI 之间的私有路由事实，不属于 Diagnostic，也不同于公开 PACK metadata 的 `owner`；它不进入公开 KAT Response，不改变“最终失败门拥有唯一 Diagnostic”的规则。Rust CLI 必须严格解码并以类型化分支保留该所有者；未知或缺失 `failure_owner` 都是 Runtime protocol failure，不能根据 Diagnostic 文本猜测。本决定不建立跨 operation 通用的 `failure_owner` 字段；其他 operation 继续服从各自既有的严格 Runtime Response，其中 ADR-0016 定义的 `test_pack` failure 仍精确只有 `status: "failure"` 与 `error`。

CLI 与 Workflow Runtime 随同一原子 KAT Payload 发布，当前没有跨版本私有 IPC 兼容承诺，因此不增加 schema version、fallback 或旧 failure 形状迁移。公开 `kat inspect --pack` failure 仍只交付一份 KAT Diagnostic 与已完整交付时的顶层 `log_path`，不会暴露内部 `failure_owner`。

本决定只修订 ADR-0010、ADR-0019、ADR-0035 与 ADR-0037 中关于 `inspect_pack` Runtime failure envelope 只有 `status` 与 `error` 的旧表述，并澄清 issue #137 中“failure 只返回 Diagnostic”约束的是公开诊断内容和禁止回显 manifest、operation、evidence path，而不是禁止该私有 envelope 携带类型化所有权；它不修订 ADR-0016 或其他 operation 的 failure 形状。
