---
status: accepted
---

# 一份 KAT diagnostic 同时驱动 JSON 与终端诊断

Runtime failure 与公开 KAT failure 共用一份稀疏 Diagnostic：非空 `message` 必填，只有可靠内容才加入近因到根因的 `causes`、可行动 `help` 和完整主 `location`；不猜测或填 null，也不加入 code、severity、traceback、私有控制事实或通用 metadata。

每次失败只由最终阻止操作成立的强制门拥有一份 Diagnostic；Runtime Diagnostic 只有在全部 CLI 外层门成功时原值进入公开 Response，后续 CLI failure 会取代而非合并它，日志、traceback 和错误字符串不用于反推字段，Operation log 是详细文本的唯一持久载体，同一净化投影仍可按操作 Interface 实时镜像到 stderr。

CLI-originated failure 冻结为同一份 typed 语义，再分别投影为 serde JSON 和 miette 终端诊断；两路不得重新解释业务结论，终端写入仅是 best-effort，也不改变最终 JSON。
