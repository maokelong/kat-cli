---
status: accepted
---

# Output Query 直接内联有界小结果

`kat query` 始终只返回 NDJSON 路径会迫使 KAT Skill 为少量元数据、聚合或有界明细再调用一次文件读取工具。Output Query 因此改为两种互斥交付：DataFusion 完整执行 SQL 并原生写出候选 NDJSON 后，同时不超过 100 个对象行和 65,536 个 payload 字节的结果以 `format="json"`、有序 `columns` 与对象 `rows` 直接进入 KAT Response；其余结果继续以 `format="ndjson"`、`columns` 与 `path` 发布。零行结果属于内联分支，小结果不保留重复文件，大结果不重复内联。

阈值只决定交付方式，不自动增加 `LIMIT`、截断、分页、超时或可配置项。Workflow Runtime 拥有文件大小检查、对象行解析和分支判定；Rust CLI 不读取查询文件、不解释 Arrow 类型或逐值重投影，只消费同版本 typed result，并把内联分支候选文件的成功删除作为 Response 发布门。候选删除失败、NDJSON 无法严格解析或任一行不是 JSON object 时整体失败，不降级或部分成功；Operation log 不复制查询行。

这是同一 KAT Skill 版本内的协调式破坏性变更，不提供 opt-in、兼容开关或 `rows`/`path` 双写。它依据 [Issue #316](https://github.com/maokelong/kat-cli/issues/316) 局部取代 ADR-0075 与 Issue #233 中“查询数据不进入 Runtime Response 或 Rust、成功结果始终只返回文件描述”的决定；DataFusion 原生 JSON 值语义、完整查询、只读 relation 边界、Session lease、无自动资源限制和大结果文件发布合同保持不变。
