---
status: accepted
---

# 当前 PACK 以 `kat.pack` 暴露，pytest 拥有测试树

`inspect_pack`、`run_workflow` 与 `test_pack` Runtime 各只选择一个 PACK，并用标准 Python 导入机制把其 canonical directory 动态挂载为公开 `kat.pack`；生产代码因此只有稳定的 `kat.pack.workflows.*` 与 `kat.pack.helpers.*` 身份，独立于 PACK name 和源码目录，而 `query_run` 不挂载 PACK，Workflow Host wheel 也只提供顶层 `kat` API。`tests/` 不属于 `kat.pack`，其模块身份、collection、fixture 与 `conftest.py` 作用域由 pytest 的 importlib 模式原生拥有，但 PACK directory 之外的父级 `conftest.py` 必须被边界排除；一个进程只复用当前 PACK 的生产模块，不建立跨 PACK namespace 或第二套测试导入语义。本决定取代 ADR-0018，并修订 ADR-0016、ADR-0017、ADR-0019 与 ADR-0045 中关于 PACK-name 私有 Python namespace、测试和生产共享 namespace、conftest 预加载以及 Workflow Host wheel 内静态 `kat.pack` 边界的旧表述；其余 Workflow discovery、测试执行、IPC 与原子发布决定保持不变。
