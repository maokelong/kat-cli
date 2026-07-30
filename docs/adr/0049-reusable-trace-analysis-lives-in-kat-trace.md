---
status: accepted
---

# 可复用 Trace 分析能力以 `kat.trace` 发布

KAT 将多个真实消费者验证过的 Trace 分析语义作为 `kat.trace` 随 Skill 和 Workflow Host wheel 原子发布，并平等提供给 Bundled 与 External PACK；它建立在稳定 Trace facts 和薄 Query Engine 之上，不建立 common PACK、PerfettoSQL loader 或新 SQL 方言。Datasource 继续拥有来源解码与规范化，`kat.trace` 拥有稳定复用算法，PACK 拥有用户问题与组织策略；同类概念以 Perfetto 语义为基线，来源差异由 Adapter 明示。候选能力须先在 PACK helper 中经真实 Trace 和至少两个真实消费者验证，未达条件的 Demo 窄移植保持私有。
