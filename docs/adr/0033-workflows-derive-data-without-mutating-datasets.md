---
status: accepted
---

# Workflow 只派生数据，不修改 Dataset

> ADR-0062 已用显式 Materialization 取代 Data Import 作为 Dataset 持久写入操作，并以 PACK 只读 Source schema 取代逐 Workflow Table Grant；本文其余只读与不修改事实输入决定继续有效。

Workflow execution plane 把 Table Grant 允许的 Dataset 表以裸名称注册为不可变 Source tables；Source 是来源与生命周期角色，不是 Workflow SQL namespace。Workflow 只能从中派生进程内 DataFrame，并通过返回值交付候选 Table Output；成功发布后它们才成为 Run Output，不能增加、更新、删除或替换 Source table。`ctx.sql` 把 SQL 交给 DataFusion 完成单语句解析、Logical Plan 构建和递归策略验证，并通过 SQL options 禁止 DDL、DML、COPY 与 session mutation；KAT 不维护自己的 SQL parser、语法白名单或语句分类，因此允许 DataFusion 接受的只读 `SHOW`、`DESCRIBE` 与 `EXPLAIN`。`ctx.sql(sql, **params)` 只把值表达式中的 `$name` 与同名关键字参数通过 DataFusion `param_values` 绑定为标量，不启用 DataFusion 的字符串替换，也不接受标识符、SQL 片段或会隐式注册临时 View 的 DataFrame 参数。Dataset 的持久写入只属于 Workflow 执行之外由用户显式发起的 Data Import，它可以按既定覆盖语义整体替换 Dataset；这避免分析策略暗中改变后续分析的事实输入，并使一次 Workflow 执行的输入边界和结果来源保持可解释。
