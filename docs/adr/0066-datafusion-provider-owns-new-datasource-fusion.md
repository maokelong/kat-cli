---
status: accepted
---

# DataFusion Provider 承接新 Datasource 融合查询

新 Datasource 不再通过 `ctx.sql()` 融合。KAT Datasource Toolkit 提供具体的 `ds.DataFusionProvider`，由 Workflow 或 PACK 像普通 Python 对象一样显式构造和调用；它不是 Provider 基类、Runtime facade 或平台发现点。一个 DataFusion Provider 可以在同一查询执行面中组合明确命名的多张内存 `ds.Table`、一个只读 Parquet Catalog，或者两者的混合，并通过 `query()` eager 返回可重复读取的 `ds.Table`。Catalog 与 Provider 的具体关系由 ADR-0067 细化。

每次查询使用隔离的短命 DataFusion Session。内存 Table 按调用开始时快照注册，Parquet 由 DataFusion 直接扫描而不预先整体加载；单语句只读 SQL、严格具名 scalar 参数和标准 Table 准入由 KAT 固定。SQL planning 完成后先按 planned result Schema 执行 D41 admission，失败时不扫描来源数据；通过后才执行和 collect，并在构造 Table 前验证实际 nullability 等数据级约束。DataFusion Provider 不发现或调用 Datasource Provider，不拆分 SQL，不执行透明远端下推，不维护全局 relation registry，也不负责来源物化、Parquet 落盘或 Run Output 发布。远端来源仍由 Workflow 先显式调用对应 Provider，所得 Table 再作为本地融合输入。Provider 的不可变 relation 绑定、复用和引用生命周期由 ADR-0068 细化，参数值域由 SDD D50 固定。

现有 `ctx.sql(sql, **params) -> DataFrame` 原样保留为旧 Dataset、`required_tables`、Table Grant 与 Execution Lease 的惰性兼容入口；它不接受新 Datasource 的 `tables`、Catalog 或 DataFusion Provider，也不改为返回 `ds.Table`。Runtime 继续负责授权和缺表检查；公共 DataFusion Provider 不接收 Context、Lease、Dataset grant 或裸 Runtime 路径。旧 Dataset 使用者迁移完成后，`ctx.sql()` 随该兼容边界删除，不为新 Datasource 保留第二套融合入口。

本决定取代 ADR-0064 中“`ctx.sql()` 是唯一 Fusion query 接口”以及立即把它破坏性迁移为新 Datasource API 的决定；ADR-0064 的显式 relation、无自动注册、短命 Session 和 eager Table 结果合同转由 DataFusion Provider 承接。ADR-0005、ADR-0032 与 ADR-0033 中 `ctx.sql(sql, **params) -> DataFrame` 的旧 Dataset 合同继续有效。它也澄清 ADR-0063：PACK 仍拥有来源特定的 Datasource Provider，而 `ds.DataFusionProvider` 是 KAT 提供的具体本地查询 Toolkit，不重新引入 KAT Provider facade、Binding 或 Provider registry。
