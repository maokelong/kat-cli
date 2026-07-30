---
status: accepted
---

# KAT 管理的分析能力需要 Execution Lease

Workflow Runtime 为每次 Workflow 执行创建与进程生命周期绑定的 Execution Lease，并通过显式 `kat.Context` 提供 `ctx.sql(...)`、`ctx.from_arrow(...)` 与 ADR-0051 收束后的 `ctx.convert_clock(...)`；每项能力都先验证 Lease，使普通 Python 直调 Workflow 或复用失效 Context 明确失败。Lease 是防误用的生命周期边界而非安全或 DRM 机制，KAT 不阻止本机用户在受支持执行面之外运行自有代码。Runtime 独占 SessionContext、catalog、UDF registry、表注册与执行生命周期；PACK 可以组合固定版本的 DataFusion DataFrame、Expr 和官方 functions，但自建 SessionContext 不会获得 KAT Dataset 或 Lease。
