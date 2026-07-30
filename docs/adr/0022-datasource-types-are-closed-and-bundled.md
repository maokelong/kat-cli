---
status: accepted
---

# Datasource types 是封闭的 KAT 能力

Datasource types 是编译进 KAT CLI、随 Platform Payload 发布的封闭强类型集合；External PACK 只能扩展 Dataset 之上的 Workflow 与领域策略，不能注册 Datasource 或 CLI 参数，因此私有格式必须先转换为受支持输入，或修改并重新发布 KAT。

预发布集合只有长期主线 Hitrace 和用于联调的 `Deprecated` Trace Streamer，不包含 Langfuse；后者读取 SQLite 全部非系统关系但不形成 SQLite Datasource、稳定 Schema 或兼容承诺，并须在首次正式发布前连同依赖它的演示与读取依赖一并迁移或删除。

这一边界以放弃动态 Datasource 插件换取 CLI 解析期的真实约束，阶段性入口不成为扩张主干能力的先例。
