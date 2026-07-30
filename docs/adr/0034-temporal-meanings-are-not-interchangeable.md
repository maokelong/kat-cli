---
status: accepted
---

# 时间语义不可互换

`ClockValue` 只有结合当前 Dataset 的 `ClockDomain` 才有完整语义；相同整数、domain 名或路径不能证明跨 Dataset 或重建前后的单位、原点和时钟实例一致，KAT 不自动对齐或持久化身份凭据，跨 Dataset 一致性由调用者负责。

第一版只提供严格字符串构造的非负 `kat.Duration` 和带明确 offset 的 `kat.WallClockTimestamp`，不提供 `kat.UnifiedTimestamp` 或 smart-time 推断；Duration 规范化为 Int64 纳秒并按 ADR 0039 无损查询，Wall-clock 规范化为 UTC RFC 3339 并与 ADR 0044 一致，原始拼写不成为持久状态。

Hitrace 事件按 ADR 0042 保留 `clock_domain + clock_value`，PACK 必须先显式换算到同一纳秒 domain 才能在验证顺序与范围后形成 Duration，且只有 `realtime` 或 `realtime_coarse` 可严格转换为 UTC timestamp；KAT 不因列名或频率猜测语义，也不增加自动差值、绝对值、降级 NULL 或平行时间 UDF。
