---
status: accepted
---

# PACK 源码布局固定

> 本文已按 ADR-0047 更新生产 Python module identity 与 pytest 测试树所有权；其余源码布局约束继续有效。

单个 PACK 目录使用固定且浅层的代码视图：可选 `workflows/` 保存 Workflow 入口，可选 `helpers/` 保存 PACK-local 普通 Python，可选 `tests/` 保存 `pytest` 测试代码及按需存在的 `datasets/` Test Dataset，PACK 根目录和测试树中可以按 pytest 规则放置 `conftest.py`。Bundled PACK 与 External PACK 都使用这一棵目录树；测试与生产源码一起版本化，不从另一个测试 root 或测试包拼装。Workflow Runtime 按可移植的相对路径顺序递归扫描 `workflows/` 中的普通 `.py` 文件；目录缺席或没有这类文件表示完整 Workflow discovery 得到零 Workflow，而不是要求 Git 保存空目录。每个入口相对路径中的目录名与 `.py` 文件 stem 都原样成为 `kat.pack.workflows.*` 下的 Python module segment，因此必须同时满足 Python 3.14 的 `str.isidentifier()` 且不满足 `keyword.iskeyword()`；KAT 不再增加 ASCII、snake_case 或其他命名规则，也不清洗、转义、散列或设置别名。非法时 inspection 直接点名该路径并失败。同一次扫描若同时发现 `workflows/cpu.py` 与 `workflows/cpu/.../*.py` 这类结构，`cpu` 会被要求同时成为普通 module 与 package；`kat.pack` 生产导入边界必须在任何入口导入前拒绝整个 PACK，并在同一诊断中指出冲突文件和目录中的代表入口。KAT 不按顺序选择一方、不生成隐藏模块名；只有目录内确实存在被扫描的 Workflow `.py` 后代时才形成冲突，普通资源目录不受限制。每个 `.py` 文件都已由位置显式声明为入口，必须恰好包含一个在本 module 定义的 `@kat.workflow(...)`；零个表示遗漏声明，多个表示入口职责混杂，均失败。其他文件类型忽略，`workflows/` 下的 `__init__.py` 则明确拒绝而不是静默跳过，因为 Python 作者会合理预期它被执行。

入口 module 不能 import 另一个 Workflow 入口，import 到的 `helpers/` module 也不能通过副作用注册 Workflow；共享实现只能放在 `helpers/`，入口身份不能从 import graph 间接产生。一次 discovery 内，Runtime 对标准 `builtins.__import__`、`importlib.__import__` 与 `importlib.import_module` 始终按当前正在加载的入口执行同一边界判断；helper 缓存这些 callable 不能保留先前入口身份，使结果依赖可移植加载顺序。这是受信任 PACK 的生产 Interface 校验，不是敌对 Python 安全沙箱；故意直接访问 `sys.modules`、替换导入设施或在导入期制造并发不属于 KAT 的隔离承诺。目录存在却无法遍历或读取、任一入口无法导入、注册来源不属于当前 module 或声明非法时，inspection 整体失败，不返回部分 Workflow。Workflow、test module 与 conftest 统一通过 `kat.pack.helpers.*` 绝对名称导入共享实现；`kat.pack.workflows.*` 只供 Runtime 标识和加载入口，不是 Workflow 间复用接口。`tests/` 只由 PACK test 交给 pytest 加载。

PACK 根目录和 `tests/` 任意适用层级的 `conftest.py` 都由 pytest 原生发现，并按目录作用域提供 fixture 与 hooks；KAT 只用 `--confcutdir=<selected-pack-directory>` 排除 PACK 目录之外的父级 conftest，不预加载或重写 PACK conftest。测试源码中的 `pytest_plugins` 不形成新的 KAT 文件扫描入口；若出现，完全服从 pytest 对绝对、可导入 module name 的标准处理，并可引用 `kat.pack.helpers.*`。`helpers/__init__.py` 可选：缺席时 `kat.pack.helpers` 使用标准 namespace package 语义，存在时作为普通 package initializer，只在首次 import 时按 Python 语义执行一次；KAT 不因扫描到文件而预加载，也不静默忽略，initializer 副作用产生的 Workflow 注册仍按来源约束拒绝。`tests/**/__init__.py`、test module 与 conftest 的加载语义属于 pytest。`workflows/` 是声明式入口树，继续拒绝任意层级的 `__init__.py`。

KAT 不再递归导入单个 PACK 目录下的所有 `**/*.py`，不支持特殊 `pack.py` 初始化入口，也不在 `pack.toml` 开放自定义扫描路径。固定目录让 Runtime 只导入已经由位置声明、并且必须在本 module 包含显式 `@kat.workflow(...)` 的入口源码，防止 inspect 误执行测试、fixture helper 或其他非入口 Python。Workflow name 只来自完整 decorator 的必填参数；文件路径、目录层级与函数名只属于实现，不参与公共身份推导。

Workflow name 在 PACK 内唯一，使用一级小写 ASCII kebab-case。PACK name 已经提供组织命名空间，因此不把递归源码目录转换成点分 Workflow namespace。移动文件或重命名函数不会改变调用 Interface；修改显式 name 是有意的破坏性变更，第一版不增加 alias 或迁移机制。每个入口文件必须恰好声明一个 Workflow；文件路径仍不成为 Workflow identity。

第一版不把 `capabilities/` 作为 KAT 入口；它与其他未识别的 PACK 根目录内容一样不扫描、不解释，也不因存在而失败。`pack.toml` 中出现 `dependencies` 则因进入 KAT 自己的封闭 manifest schema 而直接拒绝，避免作者误以为跨 PACK 依赖已经生效。
