---
status: accepted
---

# Query Result 使用 compact positional JSON

`kat query` 的成功 operation-specific `result` 始终且只包含 `dataset`、`columns` 与 `rows`。`dataset` 始终报告 Run 的 Dataset reference 及查询当下状态：Run 未提供 Dataset 时只含 `status: "not_provided"`；记录的 Dataset 当前可用时只含 `status: "available"` 与 canonical `path`；当前不可用但纯 `output.*` 查询仍成功时只含 `status: "unavailable"`、Run 记录的 `path` 与可读 `cause`。这是每次 query 的固定语义，不因 SQL 只访问 `output.*` 而省略，CLI 不解析 SQL，也不增加重复的 `current` 字段。`columns` 是按 SQL 结果顺序排列的 `{name, type}` object array，类型直接使用 Arrow 的可读字符串，不建立 KAT 类型语言；`rows` 是 positional JSON array 的 array，每行长度必须与 columns 一致，值按位置对应。Arrow `Utf8`、`LargeUtf8` 与 `Utf8View` 按 ADR 0046 原样成为 JSON string，同时保留实际 column type。Arrow `Int64` 与 `UInt64` 的非 null 值统一使用十进制 JSON string，`Int8/16/32` 与 `UInt8/16/32` 仍使用 JSON number；值是否落在 safe-integer 范围内不会改变同一 Arrow 类型的 JSON 表示。`Timestamp(ns, UTC)` 按 ADR 0044 使用规范 UTC RFC 3339 JSON string，column type 仍保留 Arrow timestamp；其他 Arrow Timestamp 严格失败。`Decimal128` 与 `Decimal256` 先复用 arrow-rs 校验值符合 precision 和 scale，再复用其 formatter 生成定点十进制 JSON string，column type 保留这两个参数；非法 Decimal 使整个 query 失败，KAT 不把它转成浮点数，也不重写 decimal 校验或格式化算法。有限浮点值使用 JSON number；任何 `NaN`、`+Infinity` 或 `-Infinity` 都使整个 query 在写 stdout 前失败，不转换为 `null` 或 string。null 对所有 nullable column 仍是 JSON `null`。Binary、Date/Time/Interval、List、Struct、Map 等尚无真实输出需求的类型严格失败，并提示 PACK 或 SQL 先显式投影为已支持标量。该形状只写一次列名和类型，同时保留列顺序与重复列名。它不增加可由 `rows.length` 推出的 `row_count`，也不携带 `truncated`、分页 token 或 artifact reference。

CLI stdout 使用普通 JSON 的 compact serialization，不输出无语义缩进或空白；pretty JSON 只用于文档。KAT 不改用 row-object JSON、column-oriented JSON、CSV、TSV、Markdown、Arrow IPC/base64 或 JSON-plus-artifact hybrid：前者重复列名，列式值要求模型跨远距离数组按下标重组记录，分隔文本需要另一套类型、null 与转义约定，二进制或 artifact 不能直接成为模型已取得的查询事实。

上下文边界主要由查询而不是压缩语法控制。Runtime 强制行数与执行时间限制，并让 `query_run` Runtime Response 的 success `result` 精确只有 `columns` 与 `rows`，不回显 CLI 已持有的 `dataset` 状态；CLI 严格解码该私有类型，加入自己的 Dataset 事实并新建完整候选 success KAT Response，并按最终 compact JSON 的 UTF-8 字节数检查输出上限，通过后才写 stdout。KAT 不按 Arrow buffer、Parquet 文件大小或特定模型 tokenizer 计算该边界。超过任一限制时整体失败，不返回部分结果、不自动注入 `LIMIT`、不截断、不分页，也不返回替代 artifact；诊断要求 Skill 缩小列、过滤、聚合或显式 `LIMIT`。无法通过小型查询获得的结构化数据应留在 Parquet，并由新增或修改的 Workflow 计算。
