---
status: accepted
---

# PACK 通过 KAT 管理的执行面转换 Arrow

非 SQL PACK 可以用 Bundled Python Host 的 PyArrow 构造 `pyarrow.Table`，但只能通过受 Execution Lease 约束的 `ctx.from_arrow(table)` 转为当前执行面的 DataFusion DataFrame；PACK 直接使用 DataFusion 的 DataFrame、Expr 与官方 functions，SessionContext、catalog、表注册和生命周期仍属 Runtime 私有。

KAT 不再包装平行算子 API，也不提供模块级或 rows、dict、pandas 等自动推断入口，以一个窄转换 Interface 支持内存算法而不公开底层执行面。
