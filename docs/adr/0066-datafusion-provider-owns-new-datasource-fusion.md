---
status: accepted
---

# DataFusion Provider 承接新 Datasource 融合查询

新 Datasource 不再通过 `ctx.sql()` 融合。KAT Data Provider Toolkit 在公共模块 `kat.dataprovider` 中提供具体的 `dp.DataFusionProvider`，由 Workflow 或 PACK 像普通 Python 对象一样显式构造和调用；它不是 Provider 基类、Runtime facade 或平台发现点。一个 DataFusion Provider 可以在同一查询执行面中组合明确命名的多张内存 `dp.Table`、一个只读 Parquet Catalog，或者两者的混合，并通过 `query()` eager 返回可重复读取的 `dp.Table`。Catalog 与 Provider 的具体关系由 ADR-0067 细化。

标准 Table 首版必须至少有一列且列名非空、唯一，只接受经锁定版本 PyArrow、DataFusion 与 Parquet 共同验证的扁平列类型：Boolean、各宽度有符号或无符号整数、Float16/Float32/Float64、Utf8/LargeUtf8/Utf8View、Binary/LargeBinary、`timestamp(ns, tz="UTC")`，以及满足 `0 <= scale <= precision` 的 Decimal128/Decimal256；字段可以 nullable，non-nullable 字段的完整 ChunkedArray 必须没有 null。union、extension、list、struct、map、date、time、duration 与其他未列类型必须由来源 Provider 或 SQL 在形成标准 Table 前显式转换。

每次查询使用隔离的短命 DataFusion Session。内存 Table 按 Provider 构造时固定的不可变绑定注册，Parquet 由 DataFusion 直接扫描而不预先整体加载；单语句只读 SQL、严格具名 scalar 参数和上述标准 Table 准入由 KAT 固定。SQL planning 完成后先校验 planned result Schema，失败时不扫描来源数据；通过后才执行和 collect，并在构造 Table 前验证实际 nullability 等数据级约束。DataFusion Provider 不发现或调用 Datasource Provider，不拆分 SQL，不执行透明远端下推，不维护全局 relation registry，也不负责来源物化、Parquet 落盘或 Run Output 发布。远端来源仍由 Workflow 先显式调用对应 Provider，所得 Table 再作为本地融合输入。Provider 的不可变 relation 绑定、复用和引用生命周期由 ADR-0068 细化。

`query(sql, *, params=None)` 的参数 Mapping 只接受精确 bool、排除 bool 且在 signed Int64 范围内的精确 int、有限的精确 float、精确 str、精确 bytes、带有效 UTC offset 的精确 `datetime.datetime`、`kat.WallClockTimestamp`、有限的精确 `decimal.Decimal` 与 signed Int64 纳秒范围内的 `kat.Duration`。KAT 分别把这些值无歧义地规范为 Boolean、Int64、Float64、Utf8、Binary、`timestamp(ns, tz="UTC")`、Decimal128/Decimal256 或 Int64 nanoseconds；datetime 按绝对 instant 规范到 UTC 纳秒，WallClockTimestamp 保留九位纳秒，Decimal 无舍入地选择满足 `0 <= scale <= precision <= 76` 的最小可容纳类型。`None`、naive datetime、NaN/Infinity、`bytearray`、`memoryview`、`date`、`timedelta`、容器、`pyarrow.Scalar` 和其他对象一律拒绝；参数错误发生在 SQL planning 前，只影响当前调用。

现有 `ctx.sql(sql, **params) -> DataFrame` 原样保留为旧 Dataset、`required_tables`、Table Grant 与 Execution Lease 的惰性兼容入口；它不接受新 Datasource 的 `tables`、Catalog 或 DataFusion Provider，也不改为返回 `dp.Table`。Runtime 继续负责授权和缺表检查；公共 DataFusion Provider 不接收 Context、Lease、Dataset grant 或裸 Runtime 路径。旧 Dataset 使用者迁移完成后，`ctx.sql()` 随该兼容边界删除，不为新 Datasource 保留第二套融合入口。

本决定取代 ADR-0064 中“`ctx.sql()` 是唯一 Fusion query 接口”以及立即把它破坏性迁移为新 Datasource API 的决定；ADR-0064 的显式 relation、无自动注册、短命 Session 和 eager Table 结果合同转由 DataFusion Provider 承接。ADR-0005、ADR-0032 与 ADR-0033 中 `ctx.sql(sql, **params) -> DataFrame` 的旧 Dataset 合同继续有效。它也澄清 ADR-0063：PACK 仍拥有来源特定的 Datasource Provider，而 `dp.DataFusionProvider` 是 KAT 提供的具体本地查询 Toolkit，不重新引入 KAT Provider facade、Binding 或 Provider registry。
