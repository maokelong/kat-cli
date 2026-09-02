---
status: accepted
---

# Catalog 从 Parquet metadata 建立只读 relation

`dp.open(root=... | tables=...)` 不接收 Datasource Schema，而是读取 Parquet footer，按文件实际物理 Schema 建立只读 `dp.Catalog`。Catalog 保存多张具名 Parquet relation、物理结构与稳定源路径，不创建或持有 DataFusion Session，不提供 `query()`，也不把文件整体读入内存。它只能作为显式 `catalog=` 输入交给 `dp.DataFusionProvider`；内存 Table 则通过独立的 `tables=` Mapping 输入。两者可以单独使用，也可以在同一个 Provider 中混合使用。

`root=` 是发现模式，只保证当前发现的非空 relation 集合各自合法，不能证明调用方设想的表集合完整；`tables=` 是显式绑定模式，要求非空 Mapping 中每个命名路径都存在并通过校验，因此知道预期表集合的 Provider 可以发现缺表。KAT 不为此引入 Datasource Schema、Manifest 或 `_SUCCESS` 标记；需要更强来源完整性的 Provider 使用显式绑定或丢弃并重建自己的可再生产物。

Catalog relation 的输入类型边界是锁定 DataFusion 能扫描的 Parquet 物理 Schema，不是 ADR-0066 的标准 `dp.Table` 准入集合。首版 footer 准入包括 Arrow 标准标量、date/time/timestamp/duration/decimal，以及递归的 list/fixed-size-list/large-list、struct、map 与 dictionary；extension、union、interval、run-end encoding、list-view 及其他未列类型拒绝。Catalog 因而可以保留 list、struct、date、duration 等更宽来源列而不先构造 Table；`DataFusionProvider.query()` 在 planning 后先校验 planned result Schema。宽 relation 的 `SELECT *` 会在扫描前失败，PACK 应显式投影、展开或 cast 成 ADR-0066 定义的标准 Table 类型。这让已有 binary Parser 的 Parquet 可以原地参与查询，同时不扩大不可变 `dp.Table`、`dp.write()` 的 Datasource Schema 校验、Run Output 和重复读取合同。

Catalog 只公开按 relation name 排序的 `catalog.tables -> tuple[str, ...]`，供调用方查看 root 发现或显式绑定的名称。它不公开 `schema()`、Relation 对象或 `pyarrow.Schema`；需要查看列名、物理类型与 nullability 时，调用方把 Catalog 交给 DataFusion Provider 并执行 `DESCRIBE <relation>`，得到普通标准 Table。这样保留 schema-less open，又不建立与 DataFusion metadata 重叠的第二套公共 Schema 模型。

`DataFusionProvider.query()` 是新 Datasource 唯一的本地 SQL 入口。它在每次调用的短命 Session 中注册 Catalog relation 和内存 Table，直接扫描 SQL 实际使用的 Parquet 数据，并 eager 返回标准 `dp.Table`。Catalog relation name 来自 `dp.open(tables=...)` 的 Mapping key 或 `dp.open(root=...)` 发现的文件 stem，内存 relation name 来自 `tables` Mapping；两个集合必须无重名，不能覆盖或 shadow。

这项拆分让 Datasource Schema 只负责 `dp.write(schema, ...)` 这一唯一 Datasource 写入事务的多 relation 逻辑结构与输入校验合同，让 Catalog 负责 Parquet 发现、路径绑定、footer 与物理 Schema admission，再由 DataFusion Provider 统一负责内存、磁盘和混合查询。打开已有文件不要求作者重复声明一份 Schema，也不增加 sidecar 或 Manifest。它细化 ADR-0066；Catalog 不提供 `query()`，远端 Datasource Provider 的来源 SQL 与迁移期旧 Dataset `ctx.sql()` 均不受其接管。
