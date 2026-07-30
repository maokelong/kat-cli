---
status: accepted
---

# Query Result 保留 Arrow 字符串内容

Skill-facing Query Result 把 Arrow `Utf8`、`LargeUtf8` 与 `Utf8View` 的非 null 内容原样投影为普通 JSON string，并保留实际列类型和 null；除标准 JSON escaping 外不截断、规范化或另加表示层。字符串及结果规模由调用方负责，KAT 不设置固定字节上限，也不静默裁剪单值或增加 `truncated` 标记。这不是通用 Arrow JSON 协议；其他类型在真实结果需要前保持不支持，由 PACK 或 SQL 显式投影为受支持标量。
