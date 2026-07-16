---
status: accepted
---

# Query Result 以规范 RFC 3339 表达 UTC 纳秒时间

Skill-facing Query Result 把 Arrow `Timestamp(ns, UTC)` 的每个非 null 值投影为 RFC 3339 JSON string。输出统一规范化到 UTC 并使用 `Z`；最多保留九位小数秒，删除无意义的尾零，整秒不输出小数部分。对应 `columns[].type` 仍保留 Arrow 的可读 timestamp 类型，null 仍是 JSON `null`。这套规范输出与 `kat.WallClockTimestamp`、Workflow 参数和 Run Manifest 共用同一个时间格式化实现，既让 Skill 与人能够直接阅读，也无损保留纳秒 instant。

第一版只支持这一种 Arrow Timestamp。其他单位、无时区或非 UTC timezone 的 Timestamp 使整个 query 失败；诊断提示 PACK 或 SQL 先使用 DataFusion 严格显式 cast 得到 `Timestamp(ns, UTC)`，确实只想展示来源文本时则显式转成 Utf8。KAT 不猜测 timezone、不使用 `try_cast` 降级为 null，也不把 epoch 整数、tagged object、额外 offset 字段或格式选项加入 Query Result。

该决定只约束 Skill-facing Query Result 的 Arrow scalar projection，不限制 Datasource 或私有 Parquet 可以承载的其他 Arrow 类型。类型不受支持或任一值无法规范格式化时，不存在部分 Query Result；当前 query 按既有整体失败语义返回 KAT diagnostic。以后只有真实查询反复需要另一种 timestamp 语义时，才增加对应的完整规则。
