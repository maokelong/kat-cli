---
status: accepted
---

# PACK 源码布局固定

Bundled PACK 与 External PACK 使用同一固定浅层源码树：`workflows/` 放入口，`helpers/` 放 PACK-local 复用实现，`tests/` 及可选 `tests/datasets/` 放随 PACK 版本化的测试；Runtime 只递归扫描 `workflows/` 的普通 `.py`，拒绝任意层级的 `__init__.py`，并要求每个文件恰好声明一个本模块 Workflow，以免 inspect 执行非入口代码。按 ADR-0047，生产代码统一挂载为 `kat.pack`，Workflow module identity 由原样满足 Python 3.14 `isidentifier()`、非 keyword 且无 module/package 冲突的相对路径段形成，不清洗或别名化；共享实现通过 `kat.pack.helpers.*` 使用，测试树由 pytest 原生拥有，公开 Workflow name 则是 PACK 内唯一的显式小写 kebab-case。KAT 不提供自定义扫描路径、`pack.py` 初始化入口或 `capabilities/` 入口，Workflow 之间不能互相导入，固定布局由此保持可预测的加载边界而不形成跨 PACK 依赖。
