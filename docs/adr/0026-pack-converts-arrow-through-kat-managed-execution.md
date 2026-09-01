---
status: superseded by ADR-0075
---

# PACK 通过 KAT 管理的执行面转换 Arrow

非 SQL PACK 算法可以直接使用 Bundled Python Host 公布的 PyArrow 构造 `pyarrow.Table`，但只能通过显式 Workflow Context 的 `ctx.from_arrow(table)` 把它转成当前受管理执行面的 DataFusion DataFrame；该 Interface 只接受 PyArrow Table，并在使用 Runtime 私有 SessionContext 前验证 Execution Lease。DataFusion 的 DataFrame、Expr 与官方 functions 是 PACK 可直接使用的公开算子面，KAT 不再包装一套 `kat.DataFrame` 或 `kat.col()`；SessionContext、DataFusion catalog、表注册和执行生命周期仍是 Runtime 私有能力，PACK 自建 SessionContext 属于不受支持的用法。KAT 也不增加模块级 `kat.from_arrow` 或 rows、dict、pandas 等自动推断入口，从而用一个窄转换 Interface 支持内存算法而不公开底层执行面。
