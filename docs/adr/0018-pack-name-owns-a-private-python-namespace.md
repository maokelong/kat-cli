---
status: superseded by ADR-0047
---

# PACK name 拥有私有 Python namespace

> 历史决定，请勿实现；当前设计见 ADR-0047。

每个精确 PACK name 在 Workflow Runtime 进程中拥有一个独立 PACK namespace。Runtime 把 PACK name 安全映射为私有合法 Python 模块作用域，并在其中加载 `workflows/`、`helpers/` 与 `tests/`。PACK name 的顶层映射仍是 Runtime 私有实现；其下用于模块身份的源码相对路径段则原样映射，并复用 Python 3.14 的 identifier 与 hard-keyword 规则，不产生第二套清洗名或 alias。每个相对路径只有一个普通 Python 模块身份；若 Workflow 扫描结果要求同一 segment 同时作为 `cpu.py` module 与 `cpu/` package，PackModuleLoader 在导入前拒绝，不以合成身份绕过 Python 语义。该 namespace 只是 Runtime 的代码隔离边界，不是 PACK 公开名称的第二层；映射名称也不是 Pack Authoring API，PACK 作者无需知道。PACK 内模块使用相对 import。

PackModuleLoader 是领域门面，不实现第二套 Python importer。它只为本次短命 Runtime 创建一个私有 root package，以标准 `ModuleSpec.submodule_search_locations` 把精确 PACK directory 作为该 package 的唯一子模块搜索位置，再通过 `importlib.import_module()` 导入已验证的入口。后续 module/package 查找、namespace package、`__init__.py`、源码编码声明、`__spec__`/`__package__`/`__file__`、缓存与 traceback 全部交给标准 `PathFinder`/`SourceFileLoader`。KAT 只保留入口扫描、路径与身份校验、模块名计算和 Workflow 注册来源校验；不读取源码后自行 `compile`/`exec`，不安装自定义 `MetaPathFinder`，不把 PACK directory 加入全局 `sys.path`，也不复制或链接源码形成临时 package tree。Bundled Python 的 `-B` 启动约束继续阻止标准 importer 在 PACK 中写入 bytecode。

Runtime 不把单个 PACK 目录加入全局 `sys.path`，并且每次 inspect、Workflow 执行或 PACK test 只加载精确选中的目标 PACK。任何跨 PACK import 都直接失败；PACK inspect、Workflow 执行与 PACK test 必须使用同一加载器，避免测试与真实运行具有不同 import 语义。

PACK test 固定使用 pytest `importlib` import mode，并由 KAT pytest plugin 的 `pytest_pycollect_makemodule` adapter 把 `tests/` 文件映射到同一私有 namespace 后交给该加载器。仅依赖 pytest 自身的路径导入会按物理目录生成另一套模块名，默认 `prepend` mode 还会修改 `sys.path`，因此两者都不能承担 PACK namespace。测试作者继续使用相对 import，例如从测试模块写 `from ..helpers.rules import normalize`；`helpers/` 与 `tests/` 无 `__init__.py` 时使用 namespace package，存在时按普通 package 语义在首次导入时执行 initializer。KAT 不要求或忽略该文件，不公开私有映射名，也不增加 `kat_pack` 等测试专用 alias。

可选的根级 `tests/conftest.py` 也先由同一 PackModuleLoader 加载，再作为 plugin object 显式交给 pytest；`--noconftest` 阻止 pytest 通过独立的物理路径加载通道再次导入它。第一版不支持嵌套 conftest：公开的 `pytest_pycollect_makemodule` 只接管测试模块，pytest 的 conftest 发现仍会按物理路径导入；直接打开会让测试与 conftest 分属两套 module identity，即使 `importlib` mode 不修改 `sys.path` 也无法消除相对 import、initializer、类型与 singleton 的分裂。KAT 不 monkeypatch pytest 的私有 conftest importer 或目录作用域状态。未来确有目录级 fixture/hook 需求时，必须重新选择统一的测试导入所有权，而不是在当前架构上暗中混用两条通道。PACK 测试源码中的 `pytest_plugins` 只透明服从 pytest 对绝对 module name 的标准导入；KAT 分配的私有 Python 顶级包名不公开，因此 PACK-local plugin 没有可依赖的字符串身份。KAT 不为它修改 `sys.path`、安装依赖，或增加 alias、finder 和第二种 PACK namespace。

同一个 `test_pack` 进程只建立并加载一次该 PACK namespace。`kat_run` 在调用间复用相同 module object，使测试中的普通 import、fixture 与 monkeypatch 不受 KAT 私有 reload 规则干扰；隔离发生在每次调用重新创建的 execution plane、Lease、Context、临时执行目录和 Output，而不是通过复制或改名 module。PACK 不得依赖 module global 保存单次 Workflow 执行状态。
