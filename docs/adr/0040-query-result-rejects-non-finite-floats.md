---
status: superseded by ADR-0075
---

# Query Result 拒绝非有限浮点值

Skill-facing Query Result 只把有限的 Arrow 浮点值投影为 JSON number。任何结果单元格为 `NaN`、`+Infinity` 或 `-Infinity` 时，整个 query 必须在写 stdout 前失败，并通过既定 KAT Response 返回可读诊断；不把特殊值改成 JSON `null`、string 或其他哨兵，也不返回其余部分。

JSON number 不包含这些特殊值。转换成 `null` 会把“存在一个非有限计算结果”篡改为“值缺失”，转换成 string 则会让同一浮点 column 的 JSON value kind 随数据变化。整体失败保持 JSON 合法、类型稳定和数据语义可见，符合 KAT 不做 best-effort 修复的原则。PACK 若希望发布其他业务语义，必须在 Workflow 或 SQL 中显式处理。

该规则只约束 Skill-facing Query Result 标量投影，不禁止 Dataset、Table Output、Parquet 或 Runtime 内部计算承载非有限浮点值，也不要求查询模块替代 PACK 判断这些值在具体业务中是否合理。
