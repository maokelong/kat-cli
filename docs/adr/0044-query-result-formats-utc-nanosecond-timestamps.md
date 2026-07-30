---
status: accepted
---

# Query Result 以规范 RFC 3339 表达 UTC 纳秒时间

Skill-facing Query Result 把 Arrow `Timestamp(ns, UTC)` 的非 null 值无损投影为规范 RFC 3339 JSON string：统一使用 UTC `Z`，最多保留九位小数并删除无意义尾零，同时保留列类型和 null；该格式与其他 KAT 墙上时间入口共用实现。其他单位、无时区或非 UTC timezone 的 Timestamp 使整个 query 失败，调用方须严格显式转换，KAT 不猜测时区、降级为 null 或增加另一种结果表示。本决定只约束 Query Result 标量投影；其他时间语义只有在真实查询反复需要时才单独决定。
