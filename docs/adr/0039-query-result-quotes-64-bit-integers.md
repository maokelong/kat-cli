---
status: accepted
---

# Query Result 用十进制字符串承载 64 位整数

Skill-facing Query Result 始终把 Arrow `Int64` 与 `UInt64` 的非 null 值投影为十进制 JSON string，同时保留列类型；较窄整数继续使用 JSON number，null 不变。统一表示既避免仅支持 binary64 的 JSON consumer 对超过 `2^53` 的值静默舍入，也避免同一 Arrow 类型随当前数值大小改变 JSON value kind。本决定只约束 Query Result 的 Arrow 标量投影，不改变 KAT Response 控制字段、私有 Runtime IPC 或其他 Arrow 类型。
