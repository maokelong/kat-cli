---
status: accepted
---

# Query Result 用十进制字符串承载 64 位整数

Skill-facing Query Result 把 Arrow `Int64` 与 `UInt64` 的每个非 null 值统一投影为十进制 JSON string，负数保留 `-`，不写前导 `+`；对应 column 的 `type` 仍为 `int64` 或 `uint64`。`Int8/16/32` 与 `UInt8/16/32` 继续使用 JSON number。null 对所有 nullable column 仍是 JSON `null`。

KAT 不在 safe-integer 范围内输出 number、超出后再切换 string；同一 Arrow 类型的 JSON value kind 不能由当前数据大小决定。统一 string 避免只支持 IEEE 754 binary64 的通用 JSON consumer 对超过 `2^53` 的时间戳、时长或计数静默舍入，列类型则保留其整数语义。每个值增加的一对引号是可接受的确定性与可移植性成本。

该决定只约束 Query Result 的 Arrow scalar projection，不把 KAT Response 控制字段或 KAT Runtime IPC 中已由双方精确约定的整数一律字符串化。其他非原生 JSON Arrow scalar 另行决定，不借本 ADR 发明通用 tagged value 或完整 Arrow JSON protocol。
