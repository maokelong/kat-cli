---
status: superseded by ADR-0075
---

# Query Result 保留 Arrow 字符串内容

Skill-facing Query Result 把 Arrow `Utf8`、`LargeUtf8` 与 `Utf8View` 的每个非 null 值都按内容原样投影为普通 JSON string，null 仍是 JSON `null`。`columns[].type` 保留当前列的实际 Arrow 类型，不因为三者具有相同 JSON value kind 就抹掉物理类型。序列化只复用标准 JSON escaping，不建立第二套转义器，也不截断、Unicode normalize、改变换行或增加 tagged object。

字符串长度继续服从完整候选 KAT Response 的既有 UTF-8 字节上限。超过上限时整个 query 失败并提示缩小投影、过滤或限制结果；KAT 不单独裁剪某个字符串，也不增加 `truncated` 标记。当前 Hitrace 与 PACK 的线程名、进程名、状态和分类等真实查询已经需要字符串族，因此这三种类型属于第一版最小标量集合。

KAT 不借此建立完整 Arrow JSON protocol。Binary、LargeBinary、FixedSizeBinary、Date、Time、Interval、List、Struct、Map 和其他尚无真实 Workflow 输出需求的类型第一版整体失败；诊断提示 PACK 或 SQL 显式投影为已经支持的标量。以后只在真实结果需要时逐种确定无损、可读且上下文成本可接受的投影规则。
