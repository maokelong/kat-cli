---
status: accepted
---

# Query Result 使用 compact positional JSON

`kat query` 的成功结果固定包含当前 Dataset reference 状态、按 SQL 顺序且保留重复名称的 Arrow `columns`，以及按位置对应的 `rows`；字符串遵循 ADR 0046，64 位整数与 Decimal 用无损十进制字符串，UTC 纳秒 timestamp 遵循 ADR 0044，有限浮点用 JSON number，其他未支持或非法值使整个查询失败。

普通 compact positional JSON 只写一次列名和类型，比 row object 少重复信息，也避免列式数组的远距离重组、分隔文本的第二套类型约定和二进制 artifact 无法直接成为模型事实。

ADR-0056 已取代固定查询限制、私有 DTO 和 CLI 平行复核：Runtime 独占 Arrow 到公开 Query Result 的投影，CLI 只附加当前 Dataset 状态；查询不设固定输出限制或 deadline，也不分页或静默截断，资源消耗由调用方负责且查询不产生持久状态。
