---
status: accepted
---

# 文件 Provider 只把旧物化结果视为可重建数据

文件 Provider 根据来源能力选择物化生命周期。没有稳定来源身份的 Provider 继续在当前 Workflow 的私有临时目录中工作；能够以来源内容建立稳定身份、并能确定性重建结果的 Provider，可以在 `ctx.datasource_root` 下跨 Workflow 复用自己的内部物化目录。该目录不进入调用方合同，旧内容只作为加速下一次查询的可重建数据，不成为系统状态或用户资产。

Provider 命中旧目录后必须重新执行自身已知的最小准入检查。打开失败、必需关系缺失或内容损坏时，Provider 丢弃该目录并从来源重建；不迁移、不回退到旧 backend，也不把一次失败继续表示为 ready。新结果先在私有临时目录完整 close，再原子发布到尚不存在的稳定目标；并发构建相同身份时，未发布的一方可以在失败后校验并采用已经成功发布的结果。

需要在对象生命周期结束时移除物化结果的调用方，由具体 Provider 提供显式临时模式。临时模式必须使用实例独占目录，不能删除或覆盖可被其他 Workflow 复用的稳定目录。KAT 不增加 Runtime 自动 close、cache registry、Manifest、迁移、回收或跨版本兼容合同；读取失败就丢弃或重建，符合可重建本地产物不承担系统状态可靠性的仓库原则。

Provider 仍是 PACK 自有普通类。`DataFusionProvider`、Catalog 和 eager `Table` 的职责不变；是否复用物化目录、如何计算来源身份以及如何校验来源参数，都由拥有来源语义的 Provider 在自己的 ADR 和测试中固定。
