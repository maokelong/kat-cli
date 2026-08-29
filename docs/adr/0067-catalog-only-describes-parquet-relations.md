---
status: accepted
---

# Catalog 从 Parquet metadata 建立只读 relation

`ds.open(root=... | tables=...)` 不接收 Datasource Schema，而是读取 Parquet footer，按文件实际物理 Schema 建立只读 `ds.Catalog`。Catalog 保存多张具名 Parquet relation、物理结构与稳定源路径，不创建或持有 DataFusion Session，不提供 `query()`，也不把文件整体读入内存。它只能作为显式 `catalog=` 输入交给 `ds.DataFusionProvider`；内存 Table 则通过独立的 `tables=` Mapping 输入。两者可以单独使用，也可以在同一个 Provider 中混合使用。

`root=` 是发现模式，只保证当前发现的非空 relation 集合各自合法，不能证明调用方设想的表集合完整；`tables=` 是显式绑定模式，要求非空 Mapping 中每个命名路径都存在并通过校验，因此知道预期表集合的 Provider 可以发现缺表。KAT 不为此引入 Datasource Schema、Manifest 或 `_SUCCESS` 标记；需要更强来源完整性的 Provider 使用显式绑定或丢弃并重建自己的可再生产物。

Catalog relation 的输入类型边界是锁定 DataFusion 能扫描的 Parquet 物理 Schema，不是标准 `ds.Table` 的 D41 准入集合。它可以保留 list、struct、date、duration 等更宽来源列而不先构造 Table；`DataFusionProvider.query()` 在 planning 后先对 planned result Schema 执行 D41 admission。因而宽 relation 的 `SELECT *` 在扫描前失败，PACK 应显式投影、展开或 cast 成标准 Table 类型。这让已有 binary Parser 的 Parquet 可以原地参与查询，同时不扩大 Python Table、append、Output 和重复读取合同。

Catalog 只公开按 relation name 排序的 `catalog.tables -> tuple[str, ...]`，供调用方查看 root 发现或显式绑定的名称。它不公开 `schema()`、Relation 对象或 `pyarrow.Schema`；需要查看列名、物理类型与 nullability 时，调用方把 Catalog 交给 DataFusion Provider 并执行 `DESCRIBE <relation>`，得到普通 D41 Table。这样保留 schema-less open，又不建立与 DataFusion metadata 重叠的第二套公共 Schema 模型。

`DataFusionProvider.query()` 是新 Datasource 唯一的本地 SQL 入口。它在每次调用的短命 Session 中注册 Catalog relation 和内存 Table，直接扫描 SQL 实际使用的 Parquet 数据，并 eager 返回标准 `ds.Table`。Catalog relation name 来自 `ds.open(tables=...)` 的 Mapping key 或 `ds.open(root=...)` 发现的文件 stem，内存 relation name 来自 `tables` Mapping；两个集合必须无重名，不能覆盖或 shadow。

这项拆分让 Datasource Schema 只负责 Python Table 的构造合同，让 Catalog 负责 Parquet 发现、路径绑定、footer 与物理 Schema admission，再由 DataFusion Provider 统一负责内存、磁盘和混合查询。打开已有文件不要求作者重复声明一份 Schema，也不增加 sidecar 或 Manifest。它细化 ADR-0066，并取代本文 SDD 先前为 `ds.Catalog.query()` 赋予本地查询能力的决定；远端 Datasource Provider 的来源 SQL 与迁移期旧 Dataset `ctx.sql()` 均不受其接管。

