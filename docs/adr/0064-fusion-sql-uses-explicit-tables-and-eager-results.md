---
status: superseded
---

# Fusion SQL 显式接收 Table 并 eager 返回 Table

> 由 ADR-0066 取代。显式 relation、短命 Session 与 eager Table 结果合同迁移到 `dp.DataFusionProvider`；`ctx.sql(sql, **params) -> DataFrame` 原样保留旧 Dataset 兼容职责。

`ctx.sql(sql, *, tables=None, params=None) -> dp.Table` 是唯一 Fusion query 接口。每次调用只把显式 `tables` Mapping 与迁移期旧 Dataset grants 注册到独立短命 Session，使用独立 `params` Mapping 绑定 scalar，完整执行后返回可重复读取的 eager `dp.Table`；调用不留下跨查询 catalog 状态。Provider query 不自动注册 relation，前一次查询结果只有再次显式传入才参与后续 Fusion query。

这项决定明确破坏性替换既有 `ctx.sql(sql, **params) -> DataFrame`，仓库内旧调用、`.collect()` 与测试必须在同一交付中迁移，不实现根据参数或调用形态改变返回类型的双模兼容。`ctx.from_arrow()` 与 DataFrame Output 可以作为另一条显式迁移路径暂时保留，但不改变 `ctx.sql()` 的单一合同。这样以一次可见迁移换取消除隐式 catalog、返回类型歧义和 Provider query 的命名副作用。

本决定取代 ADR-0005、ADR-0032 与 ADR-0033 中 `ctx.sql()` 使用 `**params` 并返回惰性 DataFrame 的部分，也取代 ADR-0062 中 Provider Table 自动注册到 operation catalog、随后由旧 `ctx.sql()` 隐式引用的部分。ADR-0032 的 Workflow Output 合同扩展为标准 `dp.Table | dict[str, dp.Table]`，迁移期继续接受旧 DataFrame 及两者组成的非空普通 dict。Execution Lease、Runtime 对 Fusion Session 的独占、旧 Dataset Table Grant、时间参数的既有 scalar 转换、Run Manifest 与 Output Query 边界保持不变。
