---
status: accepted
---

# Query Result 用定点字符串承载 Decimal

Skill-facing Query Result 把 Arrow `Decimal128` 与 `Decimal256` 的每个非 null 值投影为定点十进制 JSON string；对应 `columns[].type` 保留 precision 和 scale。KAT 不将 Decimal 转为浮点数或 JSON number，也不根据当前值的大小切换表示。null 仍为 JSON `null`。

Decimal 是由 precision 和 scale 定义的精确定点数。string 可以跨只支持 binary64 number 的通用 JSON consumer 无损传输，column type 则保留其数值类型和解释方式。额外的一对引号比静默舍入更可接受。

投影前必须先复用 arrow-rs 的 Decimal 有效性校验，保证每个非 null 值符合 column precision 和 scale；非法值使整个 query 失败。不能把 formatter 当成校验器，因为显示能力本身不负责拒绝非法底层值。通过校验后再复用 arrow-rs 已公开的 Decimal formatter：正 scale 保留对应小数位和尾随零，零 scale 输出整数，负 scale 展开为末尾零，不使用科学计数法。KAT 不复制其符号、小数点、补零或负 scale 处理算法，也不另建 Decimal parser。若所用 Arrow 版本没有可复用且满足这些语义的公共能力，应在实现切片中重新评估依赖或缩小支持范围，而不是静默引入自实现通用校验器或格式化器。

该决定只约束 Skill-facing Query Result 的 `Decimal128` 与 `Decimal256` scalar projection，不借机设计通用 Arrow JSON protocol，也不改变 Dataset、Table Output、Parquet 或私有 Runtime IPC 的表示。
