---
status: accepted
---

# 文本 Ftrace Provider 由 Pack Authoring API 公共提供

多个 PACK 需要以相同合同解码、复用并查询文本 Ftrace 时，不再复制来源 stem 校验、物化目录选择、并发发布结果复用、Clock domain 准入和 DataFusion 查询逻辑。`kat-workflow` 在 `kat.dataprovider.ftrace.FtraceProvider` 提供这一个具体公共实现；PACK 以 `source`、`clock_domain` 和当前 `ctx.datasource_root` 构造后，通过只读 `tables`、`decode_report` 与 `query()` 使用。该 Module 把重复实现收敛到一个窄 interface，不建立 Provider 基类、registry、自动来源识别或通用生命周期。

公共实现按需 import 同一 Payload 已原子安装的 `kat_datasource.text_ftrace`，两个私有 wheel 仍没有 distribution metadata dependency，也不能独立安装或升级。底层原生解码合同、Parquet relations、Source stem 物化身份和失败时不覆盖既有目录的规则不变。

Provider declaration 与 guide 继续属于 PACK。需要被 Provider inspection 发现的 PACK 在自己的 `datasources/` 中声明继承或包装公共实现的薄类，并引用 PACK 自有 guide；Workflow 也可以直接 import 公共类。公共库不携带某个 PACK 的名称、description 或 guide，其他 PACK 之间仍不发生 import 或 dependency。

本决定局部取代 ADR-0071 中普通 Python `FtraceProvider` 由单个 PACK 实现的范围，以及 ADR-0075 中 Ftrace 文本继续由具体 PACK Provider 独占实现的表述；PACK 自有 Datasource、显式构造调用、两个 wheel 的原子发布和无通用 Provider framework 继续有效。
