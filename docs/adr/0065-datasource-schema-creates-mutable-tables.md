---
status: accepted
---

# Datasource Schema 创建可追加 Table

PACK 作者通过 `dp.Schema` 一次声明多张表及其列合同，并由 `schema.create()` 得到普通 `dict[str, dp.Table]`；每个 Table 可以直接追加符合声明的数据，不另设 `TableBuilder`、`WritableTable` 或多表容器类型。`dp.Table.from_arrow()`、Source query 与 DataFusion Provider query 也返回同一种可追加 Table，不增加 ReadOnlyTable 或 QueryTable。追加合同由构造路径决定：`Schema.create()` 与 `Table(single_table_schema)` 按 Python Schema 校验，`Table.from_arrow()` 与 DataFusion Provider query 按已经准入的 Arrow 物理 Schema 校验；Source query 可以选择任一路径。查询、落盘和 Output 发布按调用时内容取得共享既有 Arrow buffers 的快照，之后 Table 仍可追加，且新增数据不反向改变既有查询结果或持久产物。这个选择接受 Table 不再等同于不可变 `pyarrow.Table`，以换取解析作者不必重复搭建列列表或理解构建阶段类型；Python 值进入 Arrow 时仍有一次必要转换，数据形成 Arrow 后 KAT 在交给融合与 Parquet 写入前不做额外整表复制。

Table 不携带固有名称。`schema.create()` 返回 Mapping 的 key 表达 Schema 表名，`dp.write()`、DataFusion Provider 和 Workflow Output 分别以调用方提供的 Mapping key 表达 Parquet 表名、短命 relation name 与 Output name；同一 Table 因而可以在不同边界使用不同名称而无需复制或修改自身。

单表也可以通过 `dp.Table(single_table_schema)` 直接创建；高级来源通过 `dp.Table.from_arrow()` 共享已有 Arrow buffers，调用时快照通过 `table.to_arrow()` 暴露。KAT 不同时保留模块级 `table()`、`from_arrow()` 或 `to_arrow()` 构造入口。

Arrow/query Table 的 append 先由 KAT 按实际物理列类型检查精确 Python 值族、范围、nullability、timestamp 和 decimal，再只把已经规范化的值交给 PyArrow 编码；不接受 PyArrow 原生允许的 float-to-int、int-to-float、bytes-to-string 或 string-to-binary 等隐式转换。整行验证成功后才进入待编码缓冲，形成快照时只增加新 Arrow chunk，不 cast、combine 或改写历史 buffers。

Schema、同步 Parquet 写入、Catalog 和已经完成的 Fusion query 都不隐藏持有调用方的输入 Table。Table 及其 Arrow buffers 只按普通 Python 强引用存活；局部构建和落盘函数返回后自然可释放，显式 `del` 不是公共工作流的一部分。

本决定取代 ADR-0064 中 `dp.Table` 不可变的部分；显式 relation、call-local Fusion Session 和 eager 查询结果合同由 ADR-0066 的 DataFusion Provider 继续承接。
