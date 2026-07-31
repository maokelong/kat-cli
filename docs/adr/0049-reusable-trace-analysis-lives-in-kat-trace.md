---
status: accepted
---

# 可复用 Trace 分析能力以 `kat.trace` 发布

KAT 把跨多个真实消费者已验证的可复用 Trace 分析语义收敛为 KAT Trace Library，以 `kat.trace` 随同一 KAT Skill 和 Workflow Host wheel 原子发布，并向 Bundled 与 External PACK 提供相同公共 Interface。它建立在 Datasource 产生的稳定 Trace facts 和薄 DataFusion Query Engine 之上，优先复用 SQL、DataFrame、PyArrow 和必要的注册 UDF；不建立可跨 PACK import 的 common PACK，也不仿造 PerfettoSQL module loader 或新 SQL 方言。

Datasource 继续拥有来源解码与规范化 facts，`kat.trace` 只拥有稳定可复用的分析能力，PACK Workflow 仍拥有用户问题和组织策略。同类 Trace 概念的名称、关系与算法语义以 Perfetto 为基线，但不依赖其私有 API 或 SQL 运行时；来源差异由 Adapter 显式表达。候选能力先在普通 PACK helper 中孵化；只有输入 facts 已稳定、语义经真实 Trace 回归验证且已有至少两个真实消费者时才晋升。`kat-openharmony-critical-path` 中尚未达到这些条件的 Perfetto 语义窄移植继续是私有候选，不因复杂或类似 Perfetto 能力就自动成为标准库。
