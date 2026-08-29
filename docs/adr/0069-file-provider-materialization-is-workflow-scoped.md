---
status: accepted
---

# 文件 Provider 物化只服务当前 Workflow

首版文件 Provider 不把 `ctx.datasource_root` 下的旧目录当作跨 Workflow cache。Workflow 在该 PACK 私有根下创建临时 workspace，把其中尚不存在的目标路径交给 Provider；Python Parser 的 Parquet、binary Parser 的数据库及其 sidecar 只在当前 Workflow 查询期间保持稳定。来源查询和本地融合 eager 返回已经脱离这些文件的 `ds.Table` 后，Workflow 清理整个 workspace。跨 Workflow 复用需要另外定义 source identity、parser/version identity、并发锁、失效和回收，不在本切片以 `exists()` 命中或固定目录覆盖代替。

文件 Provider 的 `decode()` 使用 fail-closed 状态转换：入口先清空 ready/backend，删除调用方明确交给该实例独占的旧目标；parse、write、schema-less `open()`、预期 relation 绑定与查询 backend 构造全部在局部变量中完成，只有全部成功后才一次提交。任何失败都保持未准备并尽力清理本次产物；后续 `query()` 报来源未准备，调用方可以重新 `decode()`。不回退旧 Catalog，也不允许同一 Provider 或同一独占路径上的并发 decode/query。

这个边界让 Catalog 的 live path view 在使用期间保持不变，使失败不会留下仍显示 ready 的旧 backend，也符合可重建本地产物只在当前进程生命周期内可信的仓库原则。Provider 仍是 PACK 普通类，KAT 不增加 Provider protocol、Runtime 自动 close、cache registry、Manifest 或原子发布协议；普通 Python 与 PACK pytest 可以用 `TemporaryDirectory` 或 `tmp_path` 执行同一流程。

