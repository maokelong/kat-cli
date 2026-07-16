---
status: superseded by ADR-0027
---

# PACK 依赖只访问显式导出的能力

PACK dependency 不会把被依赖 PACK 加入普通 Python import path，依赖方只能通过 Pack Authoring API 获取对方显式发布的 Exported Capabilities。这一边界提高了发布接口的设计和校验要求，但保留了未导出模块、helper 和类型的自由重构空间，并防止跨 PACK 源码 import 形成无法管理的蜘蛛网依赖。产品发布前仍允许破坏式修改 Exported Capability，但修改必须在 PACK inspect/check 阶段可见且可验证。

Exported Capability 的边界值被限制为 JSON-compatible values 与 DataFrame：函数只能接收这两类输入，并只能返回 DataFrame 或 `dict[str, DataFrame]`。PACK 内部仍可使用 dataclass 或其他领域类型，但不能把它们、callback、generator 或其他私有 Python 对象传过 PACK 边界。

为避免列名和类型变成新的隐式耦合，Exported Capability 还必须使用 Arrow/DataFusion Schema 声明 DataFrame 输入的最小必需列以及输出的确定结构，多表输出同时声明允许的 key。PACK inspect/check 校验声明和依赖引用，Workflow Runtime 在每次能力调用时验证实际输入与输出。
