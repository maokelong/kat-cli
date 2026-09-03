---
status: accepted
---

# 文件 Provider 的物化由具体 Provider 管理

文件 Provider 可以在 `ctx.datasource_root` 下保留并复用可重建的本地物化结果。物化结果
不是系统状态或用户资产，目录身份、准入检查以及读取失败后的行为由拥有来源语义的
Provider 在自己的 ADR、文档和测试中固定。

KAT 不为文件 Provider 增加统一缓存接口、生命周期方法、Manifest、迁移、自动回收或
并发协调。`DataFusionProvider`、Catalog 和 eager `Table` 的职责不变；Provider 只暴露
当前业务需要的最小接口。
