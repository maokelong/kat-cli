---
status: superseded by ADR-0079
---

# 固化 SQL 是一等 Query Asset

Data Source Knowledge 中可供复用的固化 SQL 以 `queries/<directory>/query.toml` 与同目录 `query.sql` 组成一份 Query Asset；只有同时具有这两个普通文件的一级目录才进入静态发现，其他 SQL 文件只是未受 KAT 解释的资源。Query Asset 由自己的清单提供独立于目录名的稳定身份和用途，并且只属于包含它的逻辑数据源；KAT 不从文件名或 SQL 注释猜测接口，也不把 Query Asset 提升为 Workflow 或允许其跨数据源隐式复用。

`query.toml` 的根级字段恰好是非空字符串 `name`、`title`、`description` 与可选 `parameters` table；未知字段直接失败。`name` 是所属逻辑数据源内唯一、稳定的小写 kebab-case 身份，参数名使用 snake_case，每个参数恰好声明非空 `type` 与 `description`，`type` 第一版只接受 `string`、`integer`、`number` 或 `boolean`。全部已声明参数都必填；默认值由调用 Workflow 决定并显式传入。方言从所属 Data Source Manifest 继承，输出 Schema 由实际执行和最终 Workflow Output 表达，不在 Query Asset 中重复声明。

Query Asset 执行时把参数与 SQL 分离交给数据库驱动绑定；标识符、排序表达式或任意 SQL 片段不属于值参数，KAT 不引入字符串替换式 SQL 模板语言。

分析期生成或提供的 Ad Hoc Query 可以作为本次运行输入执行，但不会写回 PACK 或自动进入 Query Asset discovery。只有 PACK 开发流程显式增加清单、SQL 和验证后，它才成为版本化 Query Asset；运行历史不充当查询资产注册表。
