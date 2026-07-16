---
status: accepted
---

# 当前 PACK 以 `kat.pack` 暴露，pytest 拥有测试树

每个短命 Workflow Runtime 进程只选择并加载一个精确 PACK。KAT 将该 PACK 的 canonical directory 挂载为稳定、公开的 Python package `kat.pack`，其中生产模块只有一组规范身份：`kat.pack.workflows.*` 与 `kat.pack.helpers.*`。`pack.toml.name` 仍是 PACK discovery、CLI、诊断和 Workflow 注册作用域中的 PACK identity，目录 basename 只表示源码位置；两者都不参与 Python module name 计算。`kat.pack` 只表示“当前 Runtime 已选择的 PACK”，不是 manifest 或 PACK object，也不提供 PACK identity、发现或选择能力。

Bundled Python Host 中的 Workflow Host wheel 提供顶层 `kat` Pack Authoring API，包括 `kat.workflow`、`kat.Context` 与领域类型，但不携带静态 `kat.pack` 实现或 PACK 源码。Runtime 在导入任何 PACK 生产代码前创建并登记动态子包 `kat.pack`，以标准 `ModuleSpec.submodule_search_locations` 将所选 PACK directory 设为唯一子模块搜索位置，再通过 `importlib.import_module()` 导入已验证的入口。后续 module/package 查找、namespace package、`__init__.py`、源码编码声明、`__spec__`、`__package__`、`__file__`、缓存与 traceback 全部交给标准 `PathFinder` 与 `SourceFileLoader`。KAT 只保留入口扫描、路径和 module identity 校验以及 Workflow 注册来源校验；不自行 `read_text + compile + exec`，不安装自定义 `MetaPathFinder`，不修改全局 `sys.path`，也不复制、链接或安装 PACK 源码。

`inspect_pack`、`run_workflow` 与 `test_pack` Runtime 都使用同一个 `kat.pack` 生产代码挂载；无目标 `kat inspect` 与 `kat inspect --dataset` 不启动这些 Runtime，`query_run` 也不选择或挂载 PACK。`kat.pack.workflows.*` 是 Runtime 标识和加载 Workflow 入口时使用的规范 module identity，不是入口之间的复用接口；Workflow 仍不得 import 另一入口，集成测试也通过 `kat_run(workflow=...)` 而非直接调用入口函数。作者文档和示例统一通过一眼可见的 `kat.pack.helpers.*` 绝对名称共享普通 Python 实现，例如 `from kat.pack.helpers.rules import normalize`；Python 原生相对 import 不另行禁止，但 KAT 不为它增加特殊语义。每个进程只有当前 PACK directory 能成为 `kat.pack`，因此不同 checkout directory 不改变生产 module identity，其他 PACK 也不在 KAT 建立的 module search path 中；代码实际引用其他 PACK 时由 Python 按普通导入规则自然失败，KAT 不静态扫描或额外拦截。若未来确实需要一个进程同时加载多个 PACK，再重新设计跨 PACK namespace，不为当前不存在的需求增加第二套名称。

`tests/` 不属于 `kat.pack`。PACK test 固定使用 pytest `--import-mode=importlib`，由 pytest 按物理测试树原生拥有测试 module name、collection、assertion rewriting、marker、参数化 node ID、fixture 与 `conftest.py` 目录作用域。PACK 根目录和 `tests/` 任意适用层级的 conftest 都按 pytest 规则生效；KAT 只传入 `--confcutdir=<selected-pack-directory>`，使用 pytest 自身的边界排除 PACK 目录之外的父级 conftest。测试通过 `kat.pack.helpers.*` 等绝对名称使用生产代码；pytest 生成的测试 module name 不是 KAT Interface，测试模块之间也不建立 KAT 专用 import 约定。只属于 pytest 的 fixture 与 hook 放在适用层级的 `conftest.py`，需要普通 Python import 的共享实现放在 `kat.pack.helpers`。

KAT 不提供 `pytest_pycollect_makemodule` adapter，不覆写 pytest 私有 `_getobj()`，不预加载 PACK 的 `conftest.py`，不传入 `--noconftest`，也不禁止嵌套 `conftest.py`。PACK 源码中的 `pytest_plugins` 完全服从 pytest 对绝对、可导入 module name 的标准语义；PACK-local plugin 可以使用 `kat.pack.helpers.*`，KAT 不扫描或改写声明、不解析相对 plugin name，也不增加 alias、finder、安装步骤或配置字段。提供 `kat_run` fixture 的 KAT plugin 仍通过 pytest 公开的 `plugins=[...]` 显式注入；它是 KAT execution seam，不是测试 module import adapter。

同一个 `test_pack` 进程只建立一次 `kat.pack`，后续 `kat_run` 调用复用相同生产 module object；pytest 独立加载测试模块不会产生第二套生产代码身份。每次调用的隔离继续来自重新创建的 execution plane、Table Grant、Execution Lease、Workflow Context、临时目录与 Output，而不是复制、重载或改名 module。`kat.pack` 只在 `inspect_pack`、`run_workflow` 与 `test_pack` Runtime 中成立；裸系统 Python 或裸 pytest 不属于受支持 Interface。

本决定取代 ADR-0018，并修订 ADR-0016、ADR-0017、ADR-0019 与 ADR-0045 中关于 PACK-name 私有 Python namespace、测试和生产共享 namespace、conftest 预加载以及 Workflow Host wheel 内静态 `kat.pack` 边界的旧表述；其余 Workflow discovery、测试执行、IPC 与原子发布决定保持不变。
