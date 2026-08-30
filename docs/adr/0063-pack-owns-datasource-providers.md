---
status: accepted
---

# PACK 拥有 Datasource Provider 与顶层源码命名空间

Datasource Provider 是 PACK 拥有并由 Workflow 直接 import、构造和调用的普通 Python 类；KAT 不再以 `SourceExecutor`、`ctx.provider()` 和不可替换的 `kat.Provider` facade 包装来源实现。PACK 固定源码布局新增可选顶层 `datasources/`，规范 module identity 为 `kat.pack.datasources.*`；Runtime 只挂载该 namespace 并服从标准 Python import，不扫描、预加载、发现或注册 Provider。KAT 改为通过独立公共模块 `kat.dataprovider` 提供可组合的 Schema、Table、`write()`、`open()`、Catalog 与具体 `DataFusionProvider` Toolkit，推荐以 `from kat import dataprovider as dp` 使用；这些能力让来源作者可以复用数据面而不把来源语义和生命周期交给平台 facade，也不再平铺到 `kat.*` 顶层。`DataFusionProvider` 是显式构造的本地查询工具，不是 Datasource Provider 基类或 KAT Provider facade。

Workflow Context 继续只读暴露受 Execution Lease 约束的 `ctx.datasource_root`，其生产值是当前 PACK 在所选 KAT Data Home 中的私有范围。Workflow 从该根派生普通 `Path` 后传给 Provider；Provider 不接收或保存 Context，Toolkit 也不读取隐式全局根。首版文件 Provider 在该根下使用当前 Workflow 的临时子目录，不把根的持久性误作跨 Workflow cache；具体 fail-closed 生命周期由 ADR-0069 固定。`kat test` 在同一 pytest test 的多次 `kat_run` 间复用该 test 的 PACK datasource root，不同 test 的根彼此隔离并在测试结束后清理，不写入生产 KAT Data Home。

这项边界选择以增加一个明确的 PACK 生产 namespace，换取来源特定的 Datasource Provider 只有一个所有者和一种面向 Workflow 的含义。继续使用 `helpers/datasources/` 不需要修改 PACK 布局，但会把稳定的来源合同误表达为无领域身份 helper；继续使用 KAT Provider facade 则会保留 factory、executor、facade 三层作者模型，并阻止 Provider 暴露来源特有的 `decode()`、`query()` 或 `materialize()`。

本决定取代 ADR-0062 中 KAT 拥有 Provider facade、`ctx.provider()` 是唯一构造入口、PACK 实现 `SourceExecutor`、Runtime 编排 executor close、Provider query 自动命名/落盘/注册，以及 operation-bound `Table` 同时承载 relation name 与 Output name 的决定，并承接其中 `ctx.datasource_root` 的 Lease、PACK 路径与测试隔离合同。本决定也取代 ADR-0032 中 Context 恰好只公开三个方法且不暴露 PACK 数据路径的部分。多数据源查询仍只组合显式来源查询结果，不因此增加透明 federation、Binding 或 Provider registry；具体 Fusion 与 eager Table 合同由 ADR-0066 承接。本决定修订 ADR-0017 与 ADR-0047 中 PACK 生产 module 只有 `workflows` 和 `helpers` 两组规范身份的决定；Workflow discovery、测试树所有权和 `kat.pack` 的单 PACK 挂载边界保持不变。
