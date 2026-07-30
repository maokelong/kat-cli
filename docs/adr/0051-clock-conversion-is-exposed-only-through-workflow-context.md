---
status: accepted
---

# 第一版时钟换算只通过 Workflow Context 暴露

本决定部分取代 ADR-0005、ADR-0009、ADR-0032、ADR-0042 与 ADR-0043 中同时开放 SQL `kat_convert_clock(...)` 和 `ctx.convert_clock(...)` 的内容，保留这些 ADR 的其余决定，且 ADR-0045 的单一纯 Python Workflow Host wheel 继续有效。第一版只通过 Workflow Context 暴露时钟换算，不注册 SQL 函数，因为 DataFusion Python API 无法可靠证明目标参数是规划期字面量，而 KAT 不为此维护第二套 parser、计划白名单或原生扩展。`ctx.convert_clock(...)` 要求精确的非空 `str` 目标，支持 `Utf8`/`LargeUtf8`/`Utf8View` domain Expr 与 `UInt64` 或可表示的非负 `Int64` value Expr，严格规范化为 `Utf8`/`UInt64` 后调用私有向量化 UDF 并返回 `UInt64` Expr；不安全转换整体失败，ADR-0042 的 null、domain、baseline 与越界语义保持不变，SQL 入口只在官方规划期检查能力和真实 PACK 需求同时出现后重议。
