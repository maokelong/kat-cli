---
status: accepted
---

# Query Result 用定点字符串承载 Decimal

Skill-facing Query Result 始终把 Arrow `Decimal128` 与 `Decimal256` 的非 null 值投影为定点十进制 JSON string，并在列类型中保留 precision 与 scale；null 不变。投影复用 arrow-rs 的有效性校验和格式化能力，非法值使整个 query 失败；string 既避免 binary64 静默舍入，也保持同一列的 JSON 类型稳定。本决定只约束 Query Result 的 Decimal 标量投影，不建立通用 Arrow JSON 协议，也不改变 Dataset、Run Output、Parquet 或私有 Runtime IPC。
