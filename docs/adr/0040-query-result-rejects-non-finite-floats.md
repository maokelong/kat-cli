---
status: accepted
---

# Query Result 拒绝非有限浮点值

Skill-facing Query Result 只把有限 Arrow 浮点值投影为 JSON number；任一单元格为 `NaN` 或正负无穷时，整个 query 失败并返回可读诊断，不改成 null、string、哨兵或部分结果。JSON number 无法表达这些值，而替代表示会混淆缺失语义或使列的 JSON 类型随数据变化；PACK 必须在 Workflow 或 SQL 中显式赋予所需业务语义。本决定只约束 Query Result 标量投影，不禁止 Dataset、Run Output、Parquet 或 Runtime 内部计算承载非有限值。
