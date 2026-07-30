---
status: superseded by ADR-0027
---

# PACK 依赖只访问显式导出的能力

历史设计不把被依赖 PACK 加入 Python import path，只允许依赖方通过 Pack Authoring API 调用显式 Exported Capabilities，以保留未导出实现的重构空间并避免源码 import 网。跨边界值只允许 JSON-compatible values 与 DataFrame，且必须用 Arrow/DataFusion Schema 声明输入最小列和确定输出，由 inspect/check 及 Runtime 校验。该依赖模型已由 ADR-0027 的第一版 PACK 完全自包含决定取代。
