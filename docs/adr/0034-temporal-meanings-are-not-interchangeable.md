---
status: accepted
---

# 时间语义不可互换

`ClockValue` 只有与同一 Dataset 中对应的 `ClockDomain` 定义结合才具有完整时间语义；相同整数不代表相同单位、原点或时钟实例。对于另一个 Dataset，或同一路径经 Data Import 重建后的 Dataset，KAT 不根据设备、时区、同名 clock domain 或相同整数自动进行对齐，也不持久化用于证明跨 Dataset 时间一致性的 token、generation 或其他身份。跨 Dataset 时钟一致性由调用者负责。

第一版 Pack Authoring API 只保留两个时间 value object：`kat.Duration` 表示不携带时间原点的非负经过时长，`kat.WallClockTimestamp` 表示带明确 UTC offset 的公历绝对时间。KAT 不提供 `kat.UnifiedTimestamp`；一个不携带 `ClockDomain` 的纳秒整数不是完整的时钟点。Hitrace 来源事件按照 ADR 0042 保留 `clock_domain + clock_value`，换算后仍以 `<target-domain>_clock_value` 一类名称表达目标 domain 上的读数。纳秒频率不会把它自动提升为第三个公共 Python value type，也不能仅凭 `timestamp_ns` 之类的列名与 Wall-clock timestamp 自动对齐。

Duration 在 Dataset 和 SQL 中以有符号 `Int64` 纳秒承载，相关列名使用 `duration_ns` 后缀；在 Pack Authoring API 中使用不可变的 `kat.Duration("5ms")`。该值保留严格构造器已经接受的原始 literal，PACK inspection 可以直接用它展示 Workflow 默认值，而不另造单位选择和可读化算法；Run Manifest 仍记录规范化的实际纳秒值，不保留等价输入拼写。有符号物理表示不扩大其非负语义：Workflow 输入与默认值中的负 Duration 必须拒绝，Datasource 不得发布负 Duration；两个时间值相减得到的普通数值也不会自动提升为 Duration。它不是普通 `int`、Python `timedelta`、Arrow Duration 或 DataFusion Expr。作为 Arrow `Int64` 出现在 Skill-facing Query Result 时，按照 ADR 0039 以十进制 JSON string 无损传输，仍保留 `int64` column type。

需要从事件时间派生 `duration_ns` 时，PACK 先通过 ADR 0042 的时钟换算把起点与终点放到同一个 `ticks_per_second = 1_000_000_000` 的 target domain，再使用 DataFusion 原生表达式计算差值。PACK 拥有“哪个是起点和终点”的业务语义，并必须显式保证终点不早于起点、结果能装入 `Int64`；通过后才使用 `duration_ns` 名称。KAT 不增加 `kat_clock_diff` 或 Duration Expr wrapper，不自动取绝对值、交换两端或把负差变成 NULL。Dataset 没有可到达的纳秒 domain 时，第一版不能从这些读数形成 KAT Duration，原始 tick 仍可用于领域明确的分析。

Wall-clock timestamp 在 Pack Authoring API 中使用不可变的 `kat.WallClockTimestamp("2026-07-14T08:30:00Z")`。Skill 与 JSON 输入边界只接受带 `Z` 或显式 offset、最多九位小数秒的 RFC 3339 字符串，解析后规范化为同一 UTC instant；规范输出始终使用 `Z`，最多保留九位小数秒并删除尾零，整秒不输出小数部分。Workflow 参数、Run Manifest 与 ADR 0044 的 Query Result 复用这套格式化规则。Dataset、SQL 和 `ctx.sql` 参数使用 Arrow `Timestamp(ns, UTC)`。它不接受无时区的 Python `datetime` 或普通整数。第一版暂不引入完整时区规则。

事件读数必须先通过 ADR 0042 显式换算到 `clock_type` 为 `realtime` 或 `realtime_coarse` 的目标 domain，PACK 才能使用 DataFusion 的严格 Arrow cast 派生 `Timestamp(ns, UTC)`；后者只降低精度。其他 domain 即使以纳秒计量，也不能据此解释为 UTC；无法转换不妨碍它们继续用于自身语义下的分析。类型越界或转换不合法时查询失败，不使用 `try_cast` 降级为 NULL。KAT 不为此增加 `origin`、`is_unix_epoch`、wall-clock UDF 或 Context 方法。

Workflow CLI 不建立统一的 smart-time parser。`kat.Duration` 接受 `[0-9]+(?:\.[0-9]{1,9})?(ns|us|ms|s|min|h)`，强制单个小写 ASCII 单位后缀，并且只在十进制定点值能够精确换算成范围内的非负整数纳秒时成功；解析不经过 float，不截断或舍入。第一版拒绝裸数字、空格、复合单位、科学计数、大小写与 Unicode 单位别名，也不要求 `duration:` 前缀。Wall-clock timestamp 使用带明确 offset、最多九位小数秒的 RFC 3339 profile，规范化为 UTC。Run Manifest 分别记录纳秒 Int64 和 UTC RFC 3339，不保留等价的用户拼写。

两个 Pack Authoring API 类型都只公开接受 `str` 的直接构造器。构造器与 CLI Adapter 共用同一严格文本解析实现；不接受 `int`、`float`、Python `datetime` 或 `timedelta`，第一版也不提供 `.parse()`、数值重载或按单位命名的平行 factory。非法文本在 PACK 模块加载时立即失败，由 KAT 保留源码位置和 cause 形成诊断。
