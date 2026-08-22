---
status: accepted
---

# PostgreSQL 时间类型首版不扩展 kat query 投影

公共 PostgreSQL common 可以把已支持的 `date32`、`time64[us]`、`timestamp[us]` 和 `timestamp[us, UTC]` 保真写入 Run Output，首个切片不因此扩展现有 `kat query` JSON 标量合同。验证这些类型时直接检查发布的 Parquet Schema 与值；需要经 `kat query` 展示时，Output Query 必须显式把当前不支持的日期时间列 cast 为 `VARCHAR`。基础标量和 Decimal 继续按既有接口直接查询。

如果用户需要 `kat query` 直接投影更多 Arrow 日期时间类型，应作为所有 Run Output 都能受益的独立接口变更设计和验证，而不是藏在 PostgreSQL common 交付中。
