---
status: accepted
---

# 文件 Provider 只把旧物化结果视为可重建数据

文件 Provider 根据来源能力选择物化生命周期。没有稳定来源身份的 Provider 继续在当前 Workflow 的私有临时目录中工作；能够以来源内容建立稳定身份、并能确定性重建结果的 Provider，可以在 `ctx.datasource_root` 下跨 Workflow 复用自己的内部物化目录。该目录不进入调用方合同，旧内容只作为加速下一次查询的可重建数据，不成为系统状态或用户资产。

Provider 命中旧目录后必须重新执行自身已知的最小准入检查。打开失败、必需关系缺失或内容损坏时，Provider 丢弃该目录并从来源重建；不迁移、不回退到旧 backend，也不把一次失败继续表示为 ready。新结果先在私有临时目录完整 close，再原子发布到尚不存在的稳定目标。

具体 Provider 可以把“是否重新解析”和“使用后是否清理”作为两个独立选项。重新解析会丢弃当前来源身份对应的旧物化，再生成新结果；清理只在 eager Table 已脱离来源后通过 Provider 的显式结束方法执行。调用方负责协调同一来源的并发查询、重新解析和清理，KAT 不增加跨 Workflow 锁、引用计数或 Runtime 自动 close。

旧目录不承担跨版本兼容或迁移责任。解码合同变化后，调用方可以显式要求重新解析；KAT 不增加持久 Schema marker、cache registry、Manifest、自动回收或迁移。Provider 仍是 PACK 自有普通类；物化身份、来源参数准入和结束语义由拥有来源语义的 Provider 在自己的 ADR、文档和测试中固定。
