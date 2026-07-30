---
status: accepted
---

# Workflow 执行能力需要显式 Workflow Context

Workflow Runtime 把受 Execution Lease 约束的 `kat.Context` 作为每个 Workflow 的显式首参，第一版只公开 `ctx.sql(...)`、`ctx.from_arrow(...)` 和 `ctx.convert_clock(...)`，其中 ADR-0051 已将时钟换算收束为仅通过 Context 暴露；模块级 `kat` 不提供隐式当前 Context，测试通过 `kat_run` fixture 执行，需要运行能力的 helper 也必须显式接收 Context。

时钟证据只在实际换算时从本次 Dataset 延迟取得并随 execution plane 释放；Context 不暴露通用 UDF、SessionContext、catalog、Dataset 路径、配置、发现或日志等能力，PACK 日志使用 Python `logging`，新增方法必须先证明是通用且受 Lease 约束的执行机制。

Workflow 只返回 DataFrame 或非空的具名 DataFrame 映射交付 Output，DataFrame 本身可以是具有确定 Schema 的零行关系，裸 DataFrame 规范化为 `main`，Output name 的可移植约束以 ADR-0055 为准，多输出逻辑上 all-or-fail；Runtime 校验实际返回值而不把 return annotation、静态 Schema 或额外描述变成第二份合同，也不为文件承诺回滚或崩溃恢复。
