---
status: accepted
---

# KAT 管理的分析能力需要 Execution Lease

Workflow Runtime 为每次 Workflow 执行创建一个与当前进程生命周期绑定的 Execution Lease，并通过显式传入 Workflow 的 `kat.Context` 提供 `ctx.sql(sql, **params)`、`ctx.from_arrow(table)` 与 `ctx.convert_clock(clock_domain, clock_value, *, target_domain)`；三个方法都在使用当前执行面前验证该 Lease。这使直接在普通 Python 中调用 Workflow 或复用失效 Context 能够立即报告“必须由 KAT 执行”，而不会误用一套不受支持的环境。Lease 是防呆和生命周期边界，不是 DRM；KAT 不声称能阻止本机用户独立安装 DataFusion 或执行其自有源码。

Workflow Runtime 拥有并且不向 PACK 暴露 DataFusion SessionContext。PACK 可以直接使用 KAT 固定版本的 DataFusion DataFrame、Expr 与官方 functions，组合 Workflow Context 返回的 DataFrame，也可以把 PyArrow Table 交给 `ctx.from_arrow(table)` 转为当前受管理执行面的 DataFrame，或通过 `ctx.convert_clock(...)` 获得调用 Runtime 已注册时钟 UDF 的 Expr；但 DataFusion catalog、UDF registry、表注册与执行生命周期仍只属于 Runtime，PACK 自建 SessionContext 不会获得 KAT Dataset 或 Execution Lease，属于不受支持的用法。
