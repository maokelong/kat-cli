# KAT 领域词汇

## KAT

Kernel AI Kit 的简称，由内核团队发起并承担平台基础设施维护责任的性能分析平台品牌；Kernel 表达项目起源、核心能力和看护责任，不把产品范围限制为内核分析。不同组织可以在同一 KAT 平台上拥有并运营整机性能、应用性能、功耗等领域的 PACK；内核团队的平台维护者角色与其 PACK 所有者角色相互独立，不赋予该 PACK 特权。

## KAT Skill

KAT 面向用户的唯一公共入口和原子发布单元，也是产品 Interface 的第一公民。用户只调用一个 `$kat` Skill 并表达自然语言目标；Skill 区分数据分析与 PACK 开发两类工作流，通过 KAT Response 取得每步成功或失败的精确结构化事实，不解析面向人的展示文案，也不把 inspect、test、run 或 query 暴露为独立 Skill。它是平台无关的一份 Skill，包含所支持平台的完整运行载荷，并在运行时自动选择当前平台入口。Skill 与其二进制载荷同版本更新；易用性和可移植性优先于发布体积。
_Avoid_: 平台专用安装包、依赖用户环境的启动脚本

## KAT analysis flow

`$kat` 面向数据使用者的主工作流。用户提供 source 和要回答的问题，Skill 内部完成 Data Import、PACK 与 Workflow 选择、执行、对 Run Output 的有界追问，最后由模型向用户给出 Analysis Result。PACK name 和 Workflow 名是高级覆盖信息，不是正常使用前提。

## Automatic Workflow selection

KAT analysis flow 根据用户问题和已导入 Dataset 自动选择 PACK 与 Workflow。Skill 先用 `kat inspect --dataset` 取得 Dataset inspection，再用无目标 `kat inspect` 执行 PACK discovery，取得只含静态 manifest 信息的 Discovered PACKs，并根据 PACK name、title 与 description 缩小候选；最后只以 `kat inspect --pack` 展开少量候选 PACK，用 Workflow 的用途、参数和 Required tables 完成匹配。只有 `required_tables` 是 Dataset 实际表集合子集的 Workflow 才能成为候选。唯一明确匹配时直接执行；只有多个候选会导致实质不同的分析方向时才询问用户。

## KAT PACK authoring flow

`$kat` 面向 PACK 作者的工作流。用户表达创建、修改、校验或测试 PACK 的目标，Skill 内部组织 PACK discovery、目标 PACK inspection、test 和错误诊断，不要求用户先选择内部操作。该工作流由 AI 分析 Workflow、SQL 和 PACK 内 helper，并在 PACK 源码中生成或修正 Required tables；运行时不回写 PACK。生产 Interface 由 inspect 校验并展开；test 在启动 pytest 前只校验生产 Interface、Test Dataset 等 KAT 自己拥有的输入边界，测试树结构与行为由 pytest 原生解释。

## PACK test

在 Bundled Python Host 中执行的标准 `pytest` 集成测试，Bundled PACK 与 External PACK 使用完全相同的测试机制。Test Dataset 是按需提供的 Workflow 测试输入，不是 PACK test 的必备结构。KAT 每个 `test_pack` Runtime 只在进程内调用一次 pytest，并通过公开的 `plugins=[...]` 显式注入携带当前 PACK 测试上下文的私有 plugin；PACK 无需主动加载 KAT plugin。Rust Dataset Storage 在启动 Runtime 前只把 `tests/datasets/` 下含有 `.kat-dataset` 条目的一级普通目录识别为 Test Dataset candidate，再解析为 Resolved Dataset；candidate 的 marker 或受管理表无效会在 pytest 启动前使 `kat test` 失败，其他内容被忽略。`tests/datasets/` 缺失、不是普通目录或没有 candidate 时得到空的 Resolved Datasets，不阻断 pytest；不调用 `kat_run` 的测试照常运行。`test_pack` request 显式携带 CLI 已选中 PACK 的 name、canonical path 和按目录名索引、允许为空的 Resolved Datasets。CLI 把子进程工作目录和 pytest rootdir 固定为当前 PACK directory，并传入 `--confcutdir=<selected-pack-directory>`；pytest 会话只读取 KAT 自带的最小配置，清除继承的 `PYTEST_*` 环境变量，关闭已安装 distribution 的 `pytest11` entry-point 自动加载和 cache plugin，因而 PACK 与父目录中的 pytest 配置以及 PACK 目录之外的父级 conftest 都不参与执行，PACK source 也不会生成 `.pytest_cache`。Runtime 在 pytest 收集前把当前 PACK 的生产代码挂载为稳定的 `kat.pack`，pytest 固定使用 `--import-mode=importlib` 并原生拥有物理 `tests/` 的 module name、collection、assertion rewriting、marker、参数化、fixture、node ID，以及 PACK 根目录和测试树各层的 `conftest.py`；KAT 不提供测试 module adapter、不预加载 conftest，也不限制嵌套 conftest。测试通过 `kat.pack.helpers.*` 等绝对名称使用生产代码，普通共享实现放在 `helpers/`，fixture 与 hook 放在适用层级的 `conftest.py`。测试请求唯一公开的 function-scoped `kat_run` factory fixture，并以注册后的 Workflow name、可选的一级 Test Dataset name 和与 `kat run` 中 `--` 后完全相同的原始 `Sequence[str]` arguments 创建独立 PACK test execution；`dataset` 可省略，省略时 plugin 不猜测或创建 Dataset；显式 name 不接受路径或运行时新增 Dataset，未被识别时即使目标 Workflow 的 Required tables 为空也报告 unknown Test Dataset。Python Runtime 在选定 Workflow 后、创建 Table Grant 前使用装饰器已规范化的 Required tables 判断本次可选 Dataset 是否足够，CLI 与 pytest plugin 不复制该语义。每次调用都经过同一个 Workflow Input Compiler、真实 Table Grant、Execution Lease 与 Output publication，再从实际 Parquet eager 读回 `dict[str, pyarrow.Table]`；已知 PACK test execution 失败直接成为带完整 KAT diagnostic 的 pytest failure，plugin 自身的意外异常原样传播。每个测试的临时执行现场位于 pytest `tmp_path` 下；KAT 固定使用 `failed` retention policy，成功测试自动清理，失败或 error 的现场沿用 pytest 默认保留最近三次测试会话，并由 `kat test` 结果打印保留根目录。多个场景使用 pytest 原生参数化；不提供 module helper、typed kwargs、`inputs={}`、Python value 到 argv 的反向序列化 Adapter、结果 wrapper、归档清理机制或 KAT 测试 DSL。

## pytest terminal report

`kat test` 直接呈现的 quiet、无 ANSI pytest 标准 terminal report。Workflow Runtime 只把这份文本写到自己的 stderr；Rust CLI 在 OS 进程边界捕获后，一边把统一净化的文本投影写入自己独占的 Operation log，一边把完全相同的文本转发到当前终端 stderr，使 PACK 作者立即看到源码位置、assertion diff 与 pytest captured output，同时保持 stdout 只含 KAT Response。测试自身的 stdout/stderr 继续服从 pytest capture 规则。KAT pytest plugin 不解释 phase、xfail 或异常类型，只通过 pytest 公开的 report hooks 取得每份 report，并调用 `pytest_report_teststatus` 复用 pytest 自己的非空 category；Runtime 内部只用 `pytest.main()` 的公开 ExitCode 选择 `test_pack` Runtime Response 分支，不序列化原始或数值 ExitCode。pytest `OK` 时，Runtime success `result` 精确为 `{"summary":{...}}`；`summary` 以 pytest category 原名为 key，只包含实际出现的正数计数并按 key 稳定排序，CLI 从这个同语义的独立 summary value 新建公开 success `result`。任何其他 ExitCode 都产生只含 `error` 的 Runtime failure，不携带 `result` 或 partial summary；`error.message` 只表达 ExitCode 已确定的失败类别，最终公开 Diagnostic 服从“最终失败门拥有”规则；完整 terminal report 及其中可复制的失败 node ID 保留在终端 stderr 与 Operation log。pytest `OK` 是唯一可进入 KAT 成功候选的退出状态，最终成功还要求 Operation log 完成交付；`NO_TESTS_COLLECTED`、collection error、测试失败与内部错误均失败，skip、xfail、xpass 以及 PACK pytest plugin 增加的 category 都保持 pytest 原生语义。发布是否需要额外测试证据不属于该操作。

## PACK Test Report

`kat test` 默认让 pytest 内建 JUnit XML reporter 生成的逐测试机器报告。它供 KAT Skill 或其他工具按需读取具体成功、失败、error 与 skipped 测试，不进入 compact `result`，也不替代面向人的 pytest terminal report 或 Operation log。每次测试使用同一个私有随机 token，把 Operation log 写到 `<data-home>/logs/test-<token>.log`，把 PACK Test Report 写到 `<data-home>/test-reports/test-<token>.xml`；token 只保证文件名唯一并关联两份证据，不形成公开 Test Run ID，KAT 也不为测试建立 registry、history 或可扫描的 Test Run 目录。CLI 分配这两个路径并准备父目录；XML 路径不进入 `test_pack` request，而像随机 Runtime Response 路径一样经私有进程启动参数交给 Runtime。Runtime 将其原样传给 pytest `--junitxml`，pytest 直接写最终随机路径；KAT 不增加临时 XML、原子重命名或发布标记，残留不会被扫描。KAT 信任成熟的 pytest reporter 对 XML 内容负责，不再次解析 XML、校验 JUnit schema 或核对测试计数，也不在私有 Runtime Response 中增加报告完成标记。`pytest.main()` 返回且 Runtime 退出后，CLI 只确认自己分配的精确路径是普通文件；成立时，无论测试 success 或 failure，KAT Response 都以顶层 `test_report_path` 引用，否则省略该字段。pytest 已经 `OK` 但文件不存在时，`kat test` 因报告未交付而失败；pytest 本身已经失败且文件不存在时保持原失败。报告不附加 captured stdout、stderr 或 logging；这些证据继续由 terminal report 与 Operation log 承担。JUnit XML 对单项 xfail/xpass 的表达不如 pytest terminal report 完整，aggregate `summary` 仍保留 pytest category。

## PACK test selector

`kat test --pack <pack-name>` 可选且可重复的 `--test <node-id>` 精确测试选择器。未提供时运行该 PACK 的完整测试集；提供时接受 pytest terminal report 中的标准 pytest node ID，例如 `tests/test_thread_cpu_time.py::test_excludes_idle`，并可原样复制失败项重跑。相对选择路径始终从所选 PACK directory 解析，与调用 KAT 时的当前目录无关；CLI 不为此改写 node ID。CLI 只在第一个 `::` 处分出路径前缀，词法检查它是当前 PACK 内以 `tests/` 开头且没有 root、Windows prefix 或 `..` 的相对平台路径；完整原始字符串随后不经重组地交给 pytest。KAT 不 canonicalize、跟随 symlink、检查选择目标是否存在，或解释 class、function、参数化 case、`[]`、逗号、Unicode 与后续 `::`。第一版不依赖 pytest 私有 parser，不透传任意 pytest argv，也不增加 `-k`、`-m` 或 `--pdb` 等并行选择和调试界面。

## PACK test isolation

单个 `test_pack` Runtime 中生产 Python module 与 pytest fixture 的会话级生命周期。KAT 只建立并加载一次 `kat.pack`，不在 `kat_run` 调用间重载或复制生产代码；pytest 以自己的 module identity 原生加载测试树，不产生第二套生产代码身份，普通 import、fixture 和 monkeypatch 因而遵循 pytest 惯例。每次 `kat_run` 仍独立创建并销毁 Workflow execution plane、Table Grant、Execution Lease、Workflow Context、logging handler、临时执行目录和 Output；调用结束后 Context 与 Lease 失效。PACK 的 Workflow 与 helper 不得把单次 Workflow execution 的可变状态保存在 module global 中。生产 `kat run` 的全新解释器语义由 KAT 自身的 CLI/IPC 端到端测试覆盖，PACK 默认测试不嵌套子进程。

## Test Dataset

Test Dataset 是 PACK 作者在测试需要运行 Workflow 时，使用正常 `kat import --dataset <tests/datasets/name> ...` 创建并随 PACK 提交的最小普通 KAT Dataset，用于重现 Workflow 必需表、Schema 和关键输入场景。`tests/datasets/` 下只有含 `.kat-dataset` 条目的一级普通目录才声明一个 Test Dataset candidate，其目录名就是 `kat_run(dataset=...)` 接受的唯一选择器；marker 一旦出现，Dataset Storage 会严格校验该 candidate，其他文件、目录和链接则不属于 Test Dataset。`tests/datasets/` 可省略；该路径缺失、不是普通目录或没有 candidate 都表示零个 Test Dataset，不使 `kat test` preflight 失败。测试不接受任意 Dataset 路径。它与用户实际运行使用完全相同的 marker、确定性 Parquet 文件树和 Rust Dataset Storage 解析校验，不存在 Test Dataset manifest、CSV 转换器或测试专用存储格式。PACK 作者可以在 authoring 阶段通过普通 Data Import 和显式 `--overwrite-dataset` 修订并重新提交它；KAT 平台执行 `kat test` 时不回写 PACK，可复现性来自 Dataset 与 PACK 一起提交，不增加 revision、snapshot 或 lock。`kat test` 在启动 Python 前一次性把全部已声明 candidate 解析为 Resolved Datasets 并交给 Runtime，空集合也合法；marker 或受管理表无效时在 pytest 前失败，未声明内容被忽略，`kat_run` 显式引用未识别 name 时报告 unknown Test Dataset。KAT 不在每次测试中重新执行 Data Import 或扫描文件树；PACK test execution 的输出写到独立临时目录。

## Skill constraints

KAT 向用户和 Agent 承诺的任务、输入和输出语义。Platform Payload、KAT CLI、Workflow Runtime 及它们的命令行参数都是该约束的私有实现，不单独承诺兼容性。
_Avoid_: 公共 CLI 接口

## Supported KAT execution

由 KAT Skill 入口启动、只使用当前 Platform Payload 的 Bundled Python Host 完成的执行。用户自行使用其他 Python 执行 PACK 不属于 KAT 执行，不提供兼容或故障支持。

## Execution Lease

Workflow Runtime 为一次 Workflow execution 创建的进程内能力凭证。它携带本次入口的 Table Grant；Workflow Context 只有持有当前有效 Lease 才能使用 `ctx.sql(...)`、`ctx.from_arrow(...)` 和 `ctx.convert_clock(...)`，其中 SQL 只能读取 Table Grant 允许的 Dataset 表；时钟换算只在本次提供了 Dataset 且其中存在 KAT 管理的 domain 定义与 baseline 时可用，未提供 Dataset 时在实际调用换算处失败。Workflow execution 结束后 Lease 失效。它用于防止误用，不是对抗本机用户的安全凭证。

## Workflow Context

Workflow Runtime 作为 Workflow 第一个参数显式传入的 `kat.Context`，是 PACK 获取当前 Execution Lease 所授予运行能力的唯一 Interface。第一版只公开 `ctx.sql(sql, **params)`、`ctx.from_arrow(table)` 和把两列 DataFusion `Expr` 换算为目标 `ClockValue Expr` 的 `ctx.convert_clock(clock_domain, clock_value, *, target_domain)`；Workflow 以 `DataFrame | dict[str, DataFrame]` 返回值交付 Output，日志使用 Python 标准库 `logging` 并由 Runtime handler 补充执行、PACK 和 Workflow 元数据。Context 不提供通用 UDF lookup、`log`、`output`、`table`、PACK 发现、配置与运行元数据读取、Dataset 路径、依赖查找或底层 SessionContext；Workflow execution 结束后失效，也不属于 Workflow 的用户输入。
_Avoid_: 隐式当前 Context、God Context、模块级执行能力

## Required tables

Workflow 通过装饰器必填的 `required_tables: list[str]` 显式声明其完整 PACK-visible Dataset 表依赖，无依赖也必须写空 list。声明覆盖 Workflow 入口及其 PACK 内私有 helper；它只表达精确的 Dataset table name，不重复声明列、类型、Datasource 身份、optional 或 alternative 分支，也不进行大小写、连字符或其他名称转换。Decorator 应用时立即复制并规范化该 list，后续修改传入对象不能改变已注册约束。这是可审查的表访问界面，不是根据某次运行生成的缓存。KAT 自有时钟操作把 `clock_domain` Source table 作为平台证据内部读取时，不把它转嫁成每个 Workflow 的声明；PACK 只有直接按表查询它时才必须显式依赖。

## Table Grant

Workflow Runtime 在选定 Workflow 后，根据其规范化 Required tables 与本次可选 Resolved Dataset 生成的进程内只读表访问授权。Required tables 非空而未提供 Dataset，或 Dataset 缺少任一所需表时，在 Workflow 调用前失败；Required tables 为空时，无论是否额外提供 Dataset 都生成空 Table Grant 并继续。Runtime 只向 Workflow execution plane 以裸表名注册 Grant 中的 Dataset 表，因此通过受支持执行面访问未声明表或改写 Source table 会失败。Table Grant 是正确性约束，不是阻止受信任本地 Python 绕过 KAT 读文件的安全沙箱。

## Source table

Table Grant 从 Dataset 暴露给 Workflow 的不可变事实输入。Source 描述其来源与生命周期，不是 Workflow SQL namespace；Workflow 以裸表名读取它。Workflow 只能从中派生新的 DataFrame 和 Run Output，不能增加、更新、删除或替换其内容；只有 Run 之外的显式 Data Import 可以整体替换 Dataset。

## Derived DataFrame

Workflow 在当前进程中通过 SQL、DataFusion DataFrame 算子或 `ctx.from_arrow(...)` 从输入派生的临时关系。它以 DataFusion DataFrame 对象流动，不注册为可由后续 SQL 寻址的 Dataset 表，也不是 Source table 或持久 Run Output；只有被 Workflow 直接返回或放入返回的具名字典后才会转换为 Table Output。

## Supported platform

KAT Skill 包含完整运行载荷并通过发布验证的目标环境。第一阶段只包含 glibc 2.28 及以上的 Linux x86_64，以及 Windows 10 及以上的 x86_64 客户端（包括 Windows 11）；musl、更旧 glibc、Windows 7/8.1、Windows Server 和其他 OS/架构在平台选择阶段明确拒绝，不回退到系统 Python、临时下载或自行编译 native wheel。

## Platform Payload

KAT Skill 中与一个 Supported platform 对应的完整私有载荷。它从自身位置发现同一 Skill 中的运行资源，可以随整个 Skill 移动，不依赖用户传入内部路径或预装运行时，也不在执行时写入或自我修改；Windows 载荷包含自身所需的 app-local 原生运行库，不要求用户预装系统级 VC Runtime、运行安装器或取得管理员权限。`kat` 或 `kat.exe` 是唯一面向用户且受支持的可执行入口；载荷内的 CPython launcher、native extensions 和 DLL 只属于 KAT 内部依赖闭包，不形成第二个产品命令。

## Platform Payload Builder

为一个 Supported platform 生成完整 Platform Payload 的构建期 Adapter。Linux 与 Windows 可以分别使用各自成熟的 Rust、Python 和原生依赖打包生态，只需共同满足同一个 Payload 目录 Interface；它们共同消费 Workflow Host wheel，并用 `uv` 把它和当前平台锁定的第三方 wheels 安装进各自的 Bundled Python Host。它们不手工复制 site-packages、不直接写 Skill deployment view，也不承担跨平台发布编排。

## Skill Assembly Adapter

把平台无关的 Skill source、Bundled PACK 与各 Platform Payload 按标准 Skill anatomy 组合为 `dist/kat` 的薄发布 Adapter，也是该目录的唯一写入者。它只理解目标路径映射，不编译 Rust、不解析 Python 依赖、不组装 CPython，也不另建版本矩阵、签名、哈希或发布一致性框架；这些通用职责交给选定的成熟构建与发布工具。

## Platform selection

KAT Skill 在每次操作前识别当前 OS、架构和必要运行约束，然后直接调用对应 Platform Payload 相对路径下的 KAT CLI：glibc 2.28 及以上的 Linux x86_64 使用 `scripts/targets/linux-x86_64/kat`，Windows 10 及以上的 x86_64 客户端使用 `scripts/targets/windows-x86_64/kat.exe`。KAT 不发布第三个“跨平台启动器”，不持久化平台选择；musl、更旧 glibc、Windows 7/8.1、Windows Server 和其他平台在启动 Payload 前以可读提示拒绝。

## Source view

KAT 仓库中表达代码职责和所有权的组织方式。它分别容纳 Skill authoring、平台机制与 Bundled PACK 源码，不复制 Skill deployment view，也不要求源码目录与 Python import namespace 同构。`kat/platform/cli` 是未来完整 CLI 的单一、不可发布 Cargo package `kat-cli`；`kat/platform/datasource` 是拥有 Dataset Storage 与全部内置 Datasource type 的单一、不可发布 library package `kat-datasource`。依赖只允许 `kat-cli` 指向 `kat-datasource`，两者随 KAT 原子发布，跨 package 的 Rust Interface 不成为 SDK 或兼容承诺；KAT 不另拆 `kat-dataset`、`storage`、`core` 或每种 Datasource 一个 package。PACK discovery 仍只是 `kat-cli` 内的 private Module，不形成公共 Rust API、独立 package、平行 `kat/platform/pack` 层、版本或部署产物。

## Skill deployment view

Skill Assembly Adapter 生成的标准 KAT Skill 目录。`SKILL.md` 和 `agents/` 定义 Skill，`scripts/targets/` 保存各平台的可执行载荷，`assets/packs/` 保存平台无关的 Bundled PACK；整个目录可以移动，不需要的标准目录不创建。

## KAT Data Home

KAT 以纯 `KAT` 为项目身份，在当前平台标准项目数据目录中使用的默认可写根，与只读 KAT Skill 分离。文档路径中的 `<data-home>` 只表示由平台目录能力解析出的这个根，不是环境变量或用户输入。其 `datasets/`、`packs/`、`runs/`、`logs/` 和 `test-reports/` 分别容纳默认 Dataset、默认 PACK、Run、Operation log 和 PACK Test Report。KAT 不再提供自有的 Data Home 环境变量、CLI 参数或配置文件覆盖；Dataset 通过自己的操作输入增加外部位置，只有无目标 `kat inspect`、`kat inspect --pack`、`kat run` 与 `kat test` 接受可重复的 `--pack-dir <directory>`，用它精确加入单个 PACK 仓库或目录，不增加用户可配置的父目录扫描入口。Run、日志和测试报告仍由 KAT 在 KAT Data Home 中创建和管理，用户不组装或指定内部路径。
_Avoid_: `kat-rs` 数据目录、维护者名数据目录、`KAT_DATA_HOME`、`--data-home`

## KAT CLI

实现 Skill 操作的单一私有短命命令行工具。源码 package `kat-cli` 最终同时包含 library implementation 与薄 binary target：前者拥有参数、目录、领域 Module、Runtime 调度与 Response 投影；唯一窄应用入口精确为 `pub fn run() -> std::process::ExitCode`，只分发已经解析的操作，并在每个 match arm 把该 handler 完成的 private `PreparedResponse<P>` 立即交给同一个 generic publisher，既不重新组合领域事实，也不建立中央 assembler registry 或全操作 result enum。后者的 `main()` 只返回 `kat_cli::run()`，作为 composition root 启动该入口，不直接调用 discovery 等内部 Module。这个 `pub` 只解决 Cargo 的 lib/bin crate 边界，`kat-cli` 仍为 `publish = false`，不形成产品、兼容 Interface 或 Rust SDK；不增加 `Application`、`Runner`、`AppContext`、万能 `run_from(args, env, io)`、公共 I/O trait 或依赖注入框架。Clap 结构直接用 `try_parse_from` 做纯解析测试，真实 binary 进程测试拥有路径、平台环境、stdout/stderr 与退出行为，只有 `response.rs` 保留私有 writer failure seam。首个完整操作切片的 `src/` 只含 Cargo 必需的 `main.rs`/`lib.rs` 与两个真实深 Module `pack_discovery.rs`/`response.rs`：当前唯一 inspect lifecycle 的 Clap、路径胶水、handler、本地 error、result DTO 与 assembler 共置在 `lib.rs`，KAT Diagnostic 与 publisher 共置在 `response.rs`；不按职责清单预拆 `args`、`dispatch`、`skill_root`、`data_home`、`inspect` 或 `diagnostic` 文件。只有第二个真实生命周期或第二个完全相同语义的调用者出现后，才按实际变化方向提取 Module，不以行数或想象中的复用拆分。只有名为 `kat` 或 `kat.exe` 的 binary 进入 Platform Payload。它的命令句法始终以 KAT 为主语，以浅层、类型明确的动词表达操作；PACK、Workflow 和 Run 只是操作的明确目标，不轮流成为多级命令主语。身份目标使用具名参数，不依赖位置顺序或复合选择器。`kat inspect` 以无目标、`--pack` 或 `--dataset` 三种互斥模式分别返回 Discovered PACKs、单个 PACK Interface 或单个 Dataset inspection，不增加平行的 `list` 动词。根命令和各操作保留 Clap 原生 `-h`/`--help`：这是尚未形成业务操作的解析期元动作，直接向 stdout 输出普通帮助文本并以 `0` 退出，不进入 KAT Response，也不解析 Skill 根、访问 KAT Data Home、执行 discovery 或创建日志。当前切片不提供 `--version`；等构建发布系统提供唯一、权威的 KAT 产品版本后再启用，不能把 workspace 临时版本冒充产品版本。裸 `kat` 没有表达任何操作，按缺少 subcommand 的 parse failure 处理；它不会用自动帮助伪装成成功。相反，裸 `kat inspect` 已经表达了既定的“列出可发现 PACK”操作，必须执行 discovery 并返回 success 或 failure KAT Response；`kat --help` 与 `kat inspect --help` 才分别是显式帮助请求。外层 KAT 参数一旦被 Clap 解析为具体 `import`、`inspect`、`test`、`run` 或 `query` 操作，该操作无论成功失败都只向 stdout 写一个 KAT Response；未知命令、缺失或冲突参数等 Clap parse failure 则保持 stdout 为空，只向 stderr 报错并以 `2` 退出。KAT 的公共进程状态只使用这一粗粒度三态：显式 help 或业务成功为 `0`，已经形成的操作 failure 以及 Response serialization/write/flush failure 为 `1`，Clap parse failure 为 `2`；领域 Module 不分配更多退出码，详细原因只进入 KAT Diagnostic。所有操作只交付 typed Response，不接触 JSON framing；唯一 generic publisher 把它以 `serde_json` compact serialization 写成一行，并统一以单个 LF 终止，不使用平台相关 CRLF、BOM、提示前缀或额外空行。pretty JSON 只用于文档示例；形成操作后不存在第二种 stdout 产品格式。实时报告和诊断的人类投影走 stderr；只有不能由结构化 Response 完整表达的可读证据才由具体操作定义为 Operation log。无目标 `kat inspect` 与 `kat inspect --dataset` 都没有这类证据，因此不创建日志。CLI 独占自己的 stdout；操作定义 Operation log 时，CLI 也独占日志并在 OS 进程边界捕获 Workflow Runtime、PACK、pytest 及其子进程继承的 stdout/stderr，把统一的可读文本投影写入日志，Runtime 不打开日志文件。`kat run` 中，CLI 还独占最终发布：Runtime 只写以 success/failure 为分支的随机临时 Runtime Response；CLI 仅在进程、Runtime Response 与日志全部完成后，才把自己持有的候选身份、PACK、Workflow 与可选 Dataset reference，同 Runtime Response 的 success result 中新产生的 effective inputs 和 Outputs 合成为不复制 Runtime Response 的 status/result wrapper 的独立 Run Manifest，再通过自己创建的同目录临时文件持久发布为唯一 `manifest.json`。只有操作明确要求实时呈现的报告才由 CLI 把同一文本投影转发 stderr。第一版不提供 `--json`、`--text`、`--format` 或 `--output` 等输出模式选项。KAT CLI 不接受泛化的执行 envelope；内部按具体操作调用 Datasource、PACK discovery、候选执行发布和 Workflow Runtime，不启动常驻服务或提供 HTTP API。
_Avoid_: REST daemon、常驻本地服务

KAT 自己解释的用户路径统一服从普通 CLI 语义：`--dataset`、`--pack-dir`、`--trace`、`--database` 等相对路径都以调用进程的当前工作目录为基准，已有目标在解析时、新目标在创建后转换为 canonical 绝对 Unicode 路径；无法取得 cwd、无法 canonicalize 或无法无损表示时由已经形成的操作报告 failure。它们不相对 Skill、binary、KAT Data Home 或 PACK。Skill 与 Platform Payload 的内部资源只从 `current_exe()` 固定层级推导，KAT Data Home 只由 `ProjectDirs` 推导，两者都不受 cwd 影响。`--test <node-id>` 是明确的 PACK-relative 测试选择器；`--` 后的 Workflow arguments 原样转发，KAT 不把其中看似路径的字符串重新解释。这些是各自 Interface 的有意边界，不建立通用 path guessing Adapter。

## KAT Response

KAT CLI 为一次外层参数已经由 Clap 解析到具体操作的调用向 KAT Skill 返回单个结构化 JSON document，也是 stdout 的唯一内容。KAT Response 是以 `status` 为分支标记的封闭 tagged union：success 分支精确包含 `status: "success"` 与操作专属 object `result`，不含 `error`；failure 分支精确包含 `status: "failure"` 与稀疏 KAT diagnostic `error`，不含 `result`。代码层以独立、封闭的 `KatResponse<P>` 复用这个外壳，`P` 是每项操作自己的具体 Skill-facing result 类型，不得以 `serde_json::Value` 或 `dict[str, Any]` 擦除类型。操作定义并成功交付 Operation log 时，相应分支额外包含顶层 `log_path`；日志未成立或文件不可读时省略。无目标 `kat inspect` 与 `kat inspect --dataset` 的两个分支都不包含 `log_path`。`kat test` 的两个分支在 pytest 返回且 CLI 分配的报告路径最终是普通文件时都额外包含顶层 `test_report_path`；它是成功与失败都可能成立的机器证据，不是 `result` 或 `error` object 的成员。`log_path` 与 `test_report_path` 在外壳中逐项显式列出，不抽象 `ResponseMeta`、通用 metadata/evidence bag 或 extension map。KAT 不使用 `error: null`、`result: {}`、`log_path: null` 或 `test_report_path: null` 占位。成功 `kat run` 的 `result` 始终且只包含 `run_id` 与非空 `outputs`：`outputs` 以已发布 Output name 为 key，每项始终且只包含 `columns` 与 `row_count`；`columns` 复用查询数据的有序 `{name, type}` object array，`row_count` 是表示完整 Output 总行数的非负 `u64` JSON number，即使为 `0` 也不省略或改成字符串。Run result 不自动附带样本行；Skill 需要数据时发起独立的有界 Output Query。failure `kat run` 没有 `result`、不发布 Run ID，并承诺没有最终 `manifest.json`。进程被外部强制终止时没有最终 KAT Response，已写日志与候选残留只供手工排障。PACK inspection 失败同样不含 `result`，也不发布 manifest、其他合法 PACK 或部分 Workflow。退出码 `0` 与 success 分支、非零退出码与 failure 分支必须分别同时成立；分支必填字段缺失、出现另一分支字段、未知字段或类型错误都属于 KAT 协议故障。顶层不重复调用方已知的 operation，不增加独立 `schema_version` 或通用 timestamp。未知 PACK、无效 Dataset、Runtime 失败和 `--` 后由 Click 拒绝的 Workflow 参数已经属于操作，必须返回 failure KAT Response；未知命令、缺失必填外层选项或其他被 Clap 拒绝的 KAT 参数尚未形成操作，只以非零退出码和 stderr 报错，不产生 JSON。KAT Response 是 CLI 从当前操作事实生成的短命产品视图，不直接透传 Runtime Response，不成为 Run Manifest 或其他持久状态，也不暴露 Output ID、物理布局和私有 IPC 字段。
_Avoid_: 终端文案解析、Runtime Response 透传、JSON lines、公共错误码

## Operation log

Operation log 是具体操作为 KAT Response 无法完整承载的可读文本证据定义的文件，不是每次 CLI 调用都要生成的审计记录。无目标 `kat inspect` 的完整事实由 `result.packs` 或 discovery typed error 表达；`kat inspect --dataset` 的完整事实由 Dataset inspection 或 Dataset Storage typed error 表达。两种操作无论成功失败都不创建 Operation log、不返回 `log_path`，也不因此创建 KAT Data Home 或 `logs/`。其他操作是否需要日志由各自 Interface 明确决定，不从动词、实现语言或是否失败隐式推导。

操作定义 Operation log 时，KAT CLI 创建、打开并独占写入一份可读文本日志；创建成功时 KAT Response 引用其路径，日志头说明操作及目标。启动 Workflow Runtime 时，CLI 在 OS 进程边界捕获 child stdout 与 stderr，包括 Python、native extension 和继承标准流的子进程输出，但日志不是原始字节归档。CLI 对两条流分别做跨 chunk 文本投影：复用成熟的流式 ANSI escape 清理实现，再按 UTF-8 增量解码；不合法字节替换为 `U+FFFD`，每条受影响的流至多加入一次清楚的 KAT 提示。CRLF 与单独 CR 统一为 LF，换行和 tab 保留，其余控制字符写成可见转义，因此日志始终是有效 UTF-8 文本；不承诺两条流的精确交错顺序。Bundled Python Host 使用 UTF-8 mode、无缓冲标准流并继承 `NO_COLOR`，但 CLI 不猜测 locale、GBK 或 native 子进程 code page，也不提供 encoding 选项、原始字节 sidecar 或 Base64 备份。畸形子进程输出本身不改变业务操作的成功与失败。已经定义的 Operation log 是系统输出的一部分；创建、任一次写入或最终 flush 失败都使当前操作失败。CLI 终止仍在运行的 Runtime、完成进程回收并拒绝接受其 Runtime Response 或发布业务成功，不在日志损坏后 best-effort 继续；即使 Runtime 已写出完整的 success 或 failure Response，也丢弃其中的结果或 Diagnostic，不据此构造公开 Response 或生产 Run Manifest，改由 CLI-owned typed log error 生成 Diagnostic。部分日志仍可读取时 failure Response 包含 `log_path`，diagnostic 明确说明日志不完整和 I/O cause；文件不可用时省略该字段。Runtime 不接收日志路径，也不直接打开日志，标准 logging 写向被捕获的 stderr。日志目录不可写、空间不足或其他原因导致创建失败时，当前操作仍返回 failure Response，省略 `log_path`，diagnostic 明确说明日志未能创建。`kat inspect --pack` 不把 PACK 输出或 traceback 回显终端，成功时 stderr 安静；合法 Runtime failure 只在本分支要求的全部 CLI-owned 交付门成功后原值移动同一 Diagnostic，Runtime 崩溃、Response 非法或任一交付门失败则由 CLI 生成自己的 Diagnostic，可读线索保留在日志。`kat test` 的独立展示规则由 CLI 把与日志完全相同的文本投影转发到终端 stderr；CLI 不从中解析结果。文件唯一性是内部实现，不形成面向用户的 Operation ID；一次 KAT Skill 请求可以引用多份 Operation logs，不与对话或外部任务身份绑定。

生产 `kat run` 在启动 Runtime 前预分配一个私有候选 UUID，并固定使用 `<data-home>/logs/run-<candidate-id>.log`；只有操作成功发布后，这个 UUID 才成为公开 Run ID，因此已发布 Run 的日志可由 `<data-home>/logs/run-<run-id>.log` 直接定位。失败结果不发布候选 ID，只通过顶层 `log_path` 引用仍可读取的日志。其他已经定义日志的操作使用各自唯一但无需从业务身份推导的文件名；这项命名规则不形成公共 Operation ID，后续 `kat query` 也不据此搜索或推断失败执行。

## KAT diagnostic

KAT 对一次失败操作给出的统一诊断语义。Skill-facing `error` 是稀疏 object：只强制一个非空 `message`，它来自该失败所有者最外层 typed error 的业务结论，不能只写 `operation failed`；只有真实且非空时才增加按近因到根因排列的 `causes`、可直接行动的 `help`，以及一个主要 `location`。公共形状不包含语义与产生方都不清晰的 `note`。CLI 自己发现的失败由共享 Diagnostic adapter 从同一份冻结的 Rust miette Diagnostic 及其真实 source chain 提取 `causes`；Runtime-originated failure 则由 Runtime 把可靠 Python exception chain 投影为同形 `causes` 后跨越 IPC，CLI 不再重建或合并。没有可靠 chain 就省略；`help` 只由确实知道恢复动作的错误变体显式提供，不自动生成泛化建议。`location` 完整包含可读逻辑输入名或 PACK 相对路径 `source`，以及从 1 开始、end-exclusive 的 `start` 和 `end` 行列；任一部分需要猜测时整体省略，不暴露私有临时路径或重复源码文本。可选字段没有内容时直接省略，不写 `null` 或空 array。`error` 不重复调用方已知的 operation 或顶层 `log_path`，也不加入 code、severity、type、retryable、traceback、底层异常类型、序列化异常对象或通用 metadata。failure Response 不存在 `result`；某项操作确实需要机器可读的失败证据时，必须显式设计该操作的 error 形状，不能扩张成无约束数据袋。操作已经定义日志但日志无法创建时，由 `message` 或已有 cause 说明原因并省略顶层 `log_path`；日志写入或 flush 中断时 diagnostic 必须说明日志不完整及可靠的 I/O cause，部分文件仍可读取便保留其顶层路径，否则同样省略。

KAT CLI 是公开 KAT Response 的唯一最终组装者。operation-specific Response assembler 与拥有该操作生命周期和强制门的 application handler 共置；领域 Module 与 Runtime client 只返回 typed facts/error，永远不依赖 KAT Response。当前切片的单个 `response.rs` 是深交付 Module：`KatResponse<P>`、KAT Diagnostic、`RenderedDiagnostic`、serde/miette 实现与 writer test seam 全部私有，只向父 `lib.rs` 暴露字段私有的 opaque `PreparedResponse<P>`，以及 `prepare_success(result)`、`prepare_cli_failure(miette::Report)`、`publish(prepared)` 三个 `pub(super)` Interface。operation-specific assembler 仍先把领域事实显式投影为自己的 concrete result，再调用前两者之一；`publish` 才消费 handoff。当前不为尚不存在的 Runtime failure、Operation log、`test_report_path` 或未来 operation 预建 Interface；真实 Runtime client 出现时才为共享 Diagnostic value 放宽必要的 crate 内可见性，`KatResponse<P>` 与 publisher implementation 不随之暴露。

`PreparedResponse<P>` 只包含已经组装的 Response 与可选 terminal projection；后者是 miette 已生成的终端 string，不保存 Report 或新增业务语义。它不进入 JSON、不是 `ResponseMeta` 或公共 metadata。publisher 先用成熟 `serde_json::to_vec` 把完整 compact Response 序列化到内存，在同一个私有 buffer 末尾追加一个 LF 形成唯一 stdout frame，再尽力写入可选 final stderr projection，最后对整帧严格 `write_all` 并 flush stdout。操作、handler 和 assembler 都不知道 serialization、LF 或 writer；不增加公共 `JsonWriter`、JSON Lines 依赖或可输出多文档的 framing abstraction。序列化失败时 stdout 保持为空；stdout 写入或 flush 失败时可能已经存在无法修复的部分 JSON；两者都只尽力向 stderr 报告 publisher failure 并以非零退出，不递归构造备用 KAT Response、重试或写第二份 JSON。terminal stderr 以及明确操作的实时 terminal mirror 都是第二公民，写入失败不改变已经确定的 Response，publisher 仍继续尝试 stdout；这不放宽 Operation log 等 Response 承诺的持久证据。stdout failure 后不回滚已经发布的 Run、Dataset 或报告，调用方必须把业务结果视为未知并将缺失、非法或与退出状态不一致的 JSON 识别为 KAT protocol failure。

CLI 不建立中央 `assemblers/` 目录、assembler registry 或一个理解所有领域类型的大 assembler `match`，顶层应用入口只保留必要的操作分发并在每个 match arm 调用同一个 generic publisher。调用点不手写序列化字段，也不存在从任意 `RuntimeResponse<R>` 到公开 Response 的 blanket `From`/`Into`、通用 conversion trait 或 generic merge。assembler 只接收 CLI-owned 与 Runtime-owned 且互不重叠的显式 typed 参数，不接收无类型 dict、通用 JSON 或两份 Response；每项事实只有一个所有者：Runtime 未知字段由严格解码判为协议错误，CLI-owned 与 Runtime-owned 的字段集合在类型定义上必须互斥；assembler 不实现运行时覆盖或字段碰撞算法；普通类型构造只是其内部实现，不形成第三个 Module。成功分支可以先构造 typed candidate 以完成验证或字节限制计算，但只有全部强制门成功后才能向 stdout 发布 success KAT Response；`kat run` 的公开 `result` candidate 必须从同一个内存 Run Manifest 纯投影并可在 persist 前构造验证，但只有最终 `manifest.json` 成功发布后才可作为 success Response 写入 stdout；该约束不泛化到 Data Import 等其他操作。失败分支只有一份最终 Diagnostic：最终阻止操作成立的强制门拥有它；合法 Runtime failure Response 且外层门全部成功时，CLI 原值移动严格解码的 Runtime Diagnostic，后续任一 CLI-owned 强制门失败则丢弃它并由 CLI typed error 生成 Diagnostic，二者从不合并或互相覆盖。

CLI-owned 领域 Module 只用 thiserror 返回保留真实 source 的 typed error，不依赖 miette；每个 operation handler 在本地定义或包装该操作自己的 CLI error，以 `thiserror::Error + miette::Diagnostic` 明确拥有用户可见的业务结论、`help` 与可靠源码位置，再把最终错误冻结为一份 owned miette Report。共享 Diagnostic adapter 只借用其中的 `&dyn miette::Diagnostic`，机械地把 `Display`、标准 `Error::source()` chain、`Diagnostic::help()` 以及唯一可靠的 primary label 与 source 投影为 KAT Diagnostic；它不导入或匹配具体领域错误，不读取 miette 的 code、severity、URL 或 related diagnostics。operation-specific CLI error 不定义这些 KAT 未采用的语义或 secondary label；SourceCode 只作为 miette 的终端排版上下文。CLI 不建立覆盖全部操作的 `CliError` 大枚举，不做 `Any` downcast、错误字符串解析，也不自造平行 Diagnostic trait。

CLI-originated failure 以这份冻结的 owned Report 为唯一语义来源：serde 序列化 adapter 从它投影出的 KAT Diagnostic，miette renderer 则直接把同一份 Report 渲染为 `RenderedDiagnostic`，保留成熟的 source chain 与源码片段能力；两条只读路径都不得重新决定 message、causes、help 或 location，不要求两个投影是同一个内存对象，也不把非序列化 render context 塞进 KAT Diagnostic DTO。已经严格解码的 Runtime Diagnostic 绕过 CLI error adapter，由 operation-specific assembler 原值装入 failure Response；需要 stderr 时只由无领域知识的通用 miette presentation adapter 把其现有字段渲染为 `RenderedDiagnostic`，IPC 没有源码正文便不显示源码片段，也不重新读取 PACK 文件猜测。handler 在 Report 仍存活时完成这两个投影，再把已组装 Response 与可选终端投影放入 private `PreparedResponse<P>`；shared publisher 不接触 Report 或重新解释 Diagnostic。Runtime 把可靠 Python exception chain 投影为 `causes`；用户可见 request fact 只有在类型化失败分支已用它直接判定失败时才可出现在既有 `message`、`causes` 或 `help` 中，候选 UUID、Runtime Response 路径、Output ID 与日志路径等私有控制事实永不进入 Diagnostic。Operation log 是 Runtime、PACK、pytest 与子进程详细文本的唯一持久证据载体；这些文本经既定净化后写入日志，不参与 Response 或 Diagnostic 组装，操作只可按明确界面把同一净化投影实时镜像到 stderr；CLI 不解析 traceback、DataFusion 错误字符串或日志来猜测结构化字段。调用点不建立 diagnostic builder 或手工拼 JSON。

## Bundled Python Host

Platform Payload 内自带的完整、可重定位 CPython 解释器、标准库和团队审查过的依赖闭包。Platform Payload Builder 将 `python-build-standalone` 的 install-only 根规范化到目标目录的 `python/`：Linux 私有 launcher 固定为 `python/bin/python3`，Windows 固定为 `python/python.exe`。它是正常的私有 Python Host，不是 venv、冻结式应用或运行时解包的单文件映像；也是 Workflow Runtime 和 Workflow execution plane 的唯一受支持宿主。KAT CLI 始终以 `-I -B -X utf8 -u -m <private-runtime-module>` 启动它并设置 `NO_COLOR`：isolated mode 忽略全部 `PYTHON*` 环境变量、当前目录、脚本目录和用户 site-packages，Runtime module 从 Host 自己的 site-packages 解析；禁止写 bytecode，因而不会在 External PACK 中产生 `__pycache__`；Python 文本 stdout/stderr 使用 UTF-8 且无缓冲。KAT 不回退到系统 Python，也不在运行时下载或安装依赖；这些启动约束只隔离 Host 初始化，不把受信任 PACK 变成沙箱代码。
_Avoid_: Python runtime、系统 Python fallback

## Workflow Host wheel

`kat/platform/workflow` 这个单一源码构建单元通过成熟 PEP 517 backend 生成的一个私有纯 Python wheel。它同时装配 PACK 使用的顶层 `kat` Pack Authoring API、公共 `kat.trace` KAT Trace Library，以及 KAT CLI 通过 `-m` 启动的私有 Workflow Runtime module；两个 Supported platform 的 Payload Builder 用 `uv` 将同一个 wheel 安装进各自 Host。该 wheel 不包含 PACK 源码或静态 `kat.pack` 实现；`kat.pack` 是 Runtime 为当前 PACK 保留并动态挂载的子包。该 wheel 只是 Source view 到 site-packages 的标准构建中间产物，不发布到包索引、不供系统 Python 安装、不提供第二个 CLI，也不让 distribution name 或版本形成独立产品 Interface。Pack Authoring API、KAT Trace Library 与 Runtime 不拆成多个 wheel，Skill Assembly Adapter 也不枚举其内部 package 文件。
_Avoid_: `kat-python-sdk`、`kat-python-runtime`、手工复制 site-packages

## Workflow execution plane

KAT 中读取 Dataset、执行 SQL 和 DataFrame 计算、运行 Workflow 并写出 Run Output 的单一 Query Engine 边界。DataFusion 只存在于拥有该边界的 Workflow Runtime；KAT CLI 与 Datasource 不链接 DataFusion，也不提供并行的 Dataset query 执行面。引擎只集中拥有 SessionContext、注册、执行生命周期、资源限制和 Table Grant 等机制；KAT 分析能力优先以 KAT Trace Library、Runtime 私有库或已注册 UDF 建在引擎之上，再通过 `kat.trace`、SQL、DataFrame Expr 或窄 Pack Authoring API 供 PACK 使用。只有代表性负载证明这层实现成为关键瓶颈时，才保持公开 Interface 和语义不变地下沉到 Rust/DataFusion 引擎层；第一版不预建双实现、FFI seam 或原生扩展。该边界产出结构化数据，不代替 LLM 或人完成最终分析。
_Avoid_: Rust query plane、双 DataFusion runtime

## Linux bundled CPython

Linux x86_64 载荷使用可重定位的 CPython 分发。它不读取用户的 Python 环境；顶层 `kat` Pack Authoring API 从 Bundled Python Host 解析，所选 PACK 则由 Runtime 挂载为 `kat.pack`。

## Trusted Python PACK code

发布 PACK 中的 Python 代码被视为受信任本地代码。KAT 不承诺对它做沙箱或细粒度权限隔离；安全边界来自团队审查、发布渠道和用户不要运行不可信来源的 Python PACK。

## Workflow Runtime

KAT 为 PACK inspect、Workflow 执行、Output Query 和 PACK test 提供的私有底层能力，也是 KAT 唯一的 DataFusion 所有者。它作为 Bundled Python Host 自身 site-packages 中的私有 module 交付，KAT CLI 只通过隔离的 `python -I -B -X utf8 -u -m <private-runtime-module>` 形式启动短命子进程；每个进程只消费一个 Runtime Request，向 CLI 指定的随机临时路径写一次 Runtime Response 后退出。只有实际执行 Workflow 或 Output Query 的路径才创建相应 execution plane；Workflow 是否携带 Dataset 不决定这个生命周期，缺少 Dataset 只意味着没有可授予的 Source table 和按需 Dataset 证据。
_Avoid_: Python runtime、Pack Worker、Pack Runner

## Runtime Request

KAT CLI 通过 `request.json` 交给单个 Workflow Runtime 进程的封闭 tagged union。它只由 CLI 根据已解析的 KAT 操作输入生成，不由 Skill、模型或用户书写；用户显式提供 Dataset 时只携带 Dataset Storage 已经解析并验证的 Resolved Dataset，而不把原始 Dataset 目录交给 Runtime 再解释；用户未提供时不由 CLI 推断 Workflow 依赖、查找默认 Dataset 或构造空 Dataset。所有跨越该 JSON 边界的文件系统路径都是由 CLI 使用平台路径能力得到的 native 绝对 Unicode 字符串；普通本地 Unicode 路径受支持，不能无损转换的 Linux 字节路径或 Windows 异常 surrogate 路径在 CLI 边界拒绝，不增加自定义字节编码或 file URI 转换。网络共享、device path 和其他非本地位置不属于受支持的 Interface，KAT 不额外识别或修补其行为。Resolved Dataset 在 request 中只编码为 `path` 与 `tables`：前者是 Dataset canonical path，后者是 Dataset table name 到已校验 canonical Parquet path 的映射，允许为空；不携带 Schema、行数、大小、Datasource、marker、相对路径或 Dataset ID。第一版只包含 `inspect_pack`、`run_workflow`、`query_run` 和 `test_pack`。`inspect_pack`、`run_workflow` 与 `test_pack` 都显式携带从本次 Discovered PACKs 中选中 PACK 的 `pack_name` 与 canonical `pack_path`：前者保留已选中的 PACK 业务身份，后者用于把当前 PACK 生产代码挂载为固定的 `kat.pack`；它们不复制 title、description、owner 或 manifest 内容，也不从进程工作目录、目录 basename 或 manifest 重新推断身份和源码位置。`run_workflow` 还携带可省略的单个 Resolved Dataset，并原样携带 CLI 未解释的 Workflow arguments；`test_pack` 还携带零个或多个通过 CLI 路径 envelope 检查后未重组的 `--test` 原始字符串，以及按一级 Test Dataset name 索引且允许为空的 Resolved Datasets；`query_run` 携带 Run ID、canonical Run path、未经 CLI 解释的 SQL、全部已发布 Output name 到 Output ID 的映射，以及始终存在、以 `status` 标记为 `not_provided`、`available` 或 `unavailable` 的 `dataset`。`not_provided` 不携带其他字段，表示该 Run 没有 Dataset reference；`available` 编码当前 Resolved Dataset 的 `path` 与 `tables`，`tables` 允许为空；`unavailable` 编码最终 `manifest.json` 记录的 canonical Dataset path 与 Dataset Storage 给出的可读 `cause`。它不携带最终 Run Manifest、Schema、行数或 Output 物理路径；Runtime 不读取 `manifest.json` 或扫描目录，只用其私有 Run Output 布局解析自己产生的 Output ID。

## Runtime Response

Workflow Runtime 针对一个 Runtime Request 向 CLI 指定的随机临时路径写入的以 success/failure 为分支的短命私有 tagged union，也是 CLI 接收 Runtime 结构化结果的唯一边界。Runtime Request 的 operation tag 只用于 Runtime 分发；Runtime Response 不包含也不回显 operation tag，CLI 根据自己发出的 Request 选择严格对应的 operation-specific Response 类型。它与 KAT Response 只复用外层词汇和分支形状而不是同一个类型：success 精确只有 `status: "success"` 与 operation-specific object `result`，不含 `error`；failure 精确只有 `status: "failure"` 与 `error` object，不含 `result`。Runtime Response 从不包含 CLI-owned `log_path` 或 `test_report_path`；CLI 必须严格解码并解构私有 Runtime 类型，再构造公开 KAT 类型；只有语义与 schema 完全相同的 Diagnostic、Column 等独立 value type 可以复用；两个 envelope 与 operation-specific result DTO 仍彼此独立，不能把 Runtime DTO、原始 JSON 或文件内容直接写到 stdout。Request 的每个 operation variant 与 Response 的每个 success/failure 分支只接受各自明确列出的字段；未知 Request operation tag、未知 Response 分支 tag、未知字段、缺少必填字段和 JSON 类型错误都使 Runtime IPC 失败，不忽略、不补默认值，也不保留 extension map。只有对应 Request variant、success result 或 failure error 明确声明为可选的字段可缺省，第一版包括 `run_workflow` request 的 Dataset 与 Runtime error 中明确声明的可选诊断字段。Runtime Response 的 success `result` 只携带 Runtime 新产生的事实，不回显 CLI 已经持有的 request facts；failure `error` 则遵循 KAT Diagnostic 的单一所有权标准：不增加 request metadata，但可在 Runtime 通过类型化控制流确认某项用户可见 request fact 直接导致本次业务失败时，在既有 `message`、`causes` 或 `help` 中引用它，私有控制事实始终禁止。`run_workflow` 的 success `result` 精确包含规范化后的 effective Workflow input values 与非空 `outputs` object；`outputs` 以合法 Output name 为 key，每个 value 精确只有 `output_id`、有序 `columns` 和 `row_count`，不重复 `name`、不承诺 Output 顺序；`columns` 精确复用 Query Result 的 `{name, type}` object array；不包含候选 UUID、PACK、Workflow 或 canonical Dataset path。CLI 通过本次随机输出路径与对应子进程生命周期关联 Request 和 Response，不依赖字段回声。Runtime Response 分支只决定 Runtime 阶段结果，最终 KAT status 仍由 CLI 在全部强制门后组装；私有 Runtime 进程退出码只表达 IPC 是否正常完成：成功写出合法 success 或 failure Response 后都以 `0` 退出；未捕获崩溃、无法写出 Response 或其他协议未完成情况才非零退出。CLI 只接受“退出码 `0` 且 Runtime Response 合法”的组合；非零退出时即使存在文件也按 Runtime protocol failure 处理，退出码 `0` 但 Response 缺失或非法同样如此。CLI 通过 Runtime 启动控制面提供一个不属于 request variant 的随机临时 Runtime Response 输出路径。Runtime 无法解码 request 时在该位置已知的前提下写通用 failure；CLI 无法取得合法 Runtime Response 时生成 Runtime protocol failure。该私有协议只服务同一原子包内的 CLI 与 Runtime，第一版没有 `schema_version`、版本协商、迁移或 fallback，也不额外约束字段顺序、空白和重复 key。

代码层以独立、封闭的 `RuntimeResponse<R>` 复用私有 IPC 的 success/failure 外壳；`R` 是 CLI 根据已知 Request 选择的 operation-specific concrete result 类型，不得以 `serde_json::Value`、`dict[str, Any]` 或 extension map 擦除类型。Runtime Response 只是 CLI 最终组装器的一项严格 typed 输入，不是公开 Response 的半成品。success result 只提供 Runtime-owned facts，由 operation-specific Response assembler 与 CLI-owned facts 组合；Runtime 未知字段在严格解码时失败，两侧字段集合在类型定义上互斥，assembler 不接受通用 JSON、字段 merge 或值层碰撞检查；`RuntimeResponse<R>` 与 `KatResponse<P>` 没有通用转换。failure 只按 KAT Diagnostic 的最终失败门规则选择一份 Diagnostic。

_Avoid_: Runtime summary、内部 summary、临时 summary

## PACK inspect

`kat inspect --pack <pack-name>` 是目标 PACK 生产 Interface 的权威校验与展开入口，并使用 `inspect_pack` request 加载、注册和验证 Workflow。CLI 独占 manifest 解析，只把已选中 PACK 的 name 与 canonical path 交给 Runtime；Runtime 不读取 `pack.toml`，只导入 `workflows/` 中的入口源码，不创建 Workflow execution plane，并在私有 Runtime Response 的 success `result` 中返回完整 `workflows`。CLI 验证后将其与 PACK discovery 已校验的所选 PACK `name`、`title`、`description`、`owner` 合成公开 `result`，该结果直接是 PACK object，不再增加 `pack` wrapper。Workflow 按 name 排序，每项包含 `name`、`title`、`description`、按 name 去重排序的 `required_tables`，以及按函数签名顺序排列的 `parameters`。parameter 固定包含 `name`、Compiler 生成的真实 `option`、`type`、`required` 和 `description`；bool 额外包含 `negative_option`，字符串 Literal 额外包含按 Bundled Python 普通字符串顺序去重排序的非空 `choices`；默认选择仍只由独立 `default` 表达。必填参数省略 `default`，可选参数始终携带 `default`，包括 `false`、空字符串与允许的 JSON `null`。`type` 只取 `string`、`int64`、`float64`、`boolean`、`duration` 与 `wall_clock_timestamp`。`default` 只投影与 argv 共用的 Click 类型转换后得到的 effective default：`string`/Literal 使用 JSON string，`int64` 使用无损十进制 JSON string，有限 `float64` 使用 JSON number，`boolean` 使用 JSON boolean，Duration 使用 temporal `ParamType` 保留的合法 literal，WallClockTimestamp 使用规范 UTC RFC 3339，Optional None 使用 JSON `null`。完整 PACK inspection 没有发现 Workflow 时返回成功且 `workflows: []`；只有 manifest、导入或声明错误导致无法形成可信完整结果时才失败；failure Response 不含 `result`，也不返回 manifest 或部分 Workflow。Skill 直接使用这些 option，不从 Python 参数名推导 flag，也不解析 Python signature、Click help 或 JSON Schema；没有覆盖参数时省略 option，让 Runtime 使用真实默认值，空 PACK 则自然不进入 Workflow 候选。`kat run` 与 `kat test` 加载目标 PACK 时复用同一套生产 Interface 约束，不要求事先执行 inspect。这里列出的 Workflow name 就是 `kat run --workflow` 与 `kat_run(workflow=...)` 接受的精确选择器；`helpers/` 和 `tests/` 不参与入口扫描。无目标 `kat inspect` 只由 KAT CLI 执行 PACK discovery 并输出 Discovered PACKs，成功 `result` 固定为 `{"packs":[...]}`，不发送该请求或导入 Python；任一 candidate 损坏时整个 discovery 失败并返回不含 `result` 的 failure Response，也不发布其他合法 PACK。

## KAT Runtime IPC

KAT CLI 与 Workflow Runtime 之间的内部文件式 IPC 边界。CLI 独占 `request.json` 的生成权、Operation log 和 Runtime 进程标准流；request 不携带 log path。CLI 使用成熟的临时文件能力为每次调用分配私有随机 Runtime Response 路径，并通过 Runtime 启动控制面提供；该 IPC 文件不需要与最终 `manifest.json` 位于同一目录，只有 CLI 后续创建的 Run Manifest 临时文件必须与最终文件同目录。Runtime 只向该路径写一次 Runtime Response 后退出，普通 logging 与 PACK 输出写向被 CLI 捕获的 stderr/stdout。Operation log 任一次写入或最终 flush 失败时，CLI 终止仍在运行的 Runtime、始终完成回收，不接受已经产生或随后产生的 Runtime Response，当前操作只能失败。Runtime Response 的 failure `error` 只携带 Runtime 真正知道的诊断事实，不承诺稳定的公共错误码。它不把候选 UUID、Runtime Response 路径、Output ID 或日志路径等私有控制事实写进 Diagnostic，也不把 PACK、Workflow、Dataset 等用户可见 request facts 当作字段或固定上下文自动回显；只有类型化 Runtime 分支已经用某项用户事实判定当前业务 failure 时，才可在既有 `message`、`causes` 或 `help` 中引用它。内部 Runtime Response 只供 CLI 验证和生成 KAT Response，不能直接作为 Skill 输出，也不能形成 Run。`inspect_pack` Runtime Response 的 success 与 failure 互斥，只有 success `result` 携带完整 `workflows` array，failure 不携带部分 Workflow；Runtime 不解析或回显 manifest fields。CLI 将已验证 workflows 与 PACK discovery 已校验的所选 PACK 静态 manifest 字段合成完整公开 PACK object；Runtime 非零退出，或者退出码 `0` 但 Runtime Response 缺失、非法时，由 CLI 判为 Runtime 协议故障，公开 failure Response 不含 `result`。候选执行的 request 只保留为该次执行输入和诊断证据，后续操作不从中重建已发布 Run。CLI 排空标准流、回收 Runtime、确认退出码为 `0` 并严格验证 Runtime Response 的 success 分支，并完整写入及 flush Operation log 后，才把自己持有的候选 UUID、PACK、Workflow 与可选 canonical Dataset path，同已验证 Runtime `result` 中的 effective inputs 和 Outputs 合成为不复制 Runtime Response 的 status/result wrapper 的 Run Manifest，写入 CLI 自己创建的同目录临时文件，再以成熟的临时文件持久化能力发布为最终 `manifest.json`；此时候选 UUID 与目录才成为可查询的 Run。Runtime Response 不会被直接重命名为 Run Manifest。任一步失败都不产生 Run 或最终文件，清理失败留下的随机临时残留也永远不被识别。Output Query 只由 CLI 读取这个精确最终文件；它把已定位的 canonical Run path、其中的 Output name 到 Output ID 映射，以及从可选 Dataset path 派生的三态 Dataset 组合成当前短命 `query_run` request，这份派生副本不是第二个持久 Run Manifest。Runtime 不读取 Data Home 或最终 `manifest.json`，也不扫描 Run 或 Dataset 目录；Run Output 的 ID 到物理文件规则只属于 Runtime 自身实现。`test_pack` Runtime 只用 pytest 公开 ExitCode 选择 Response 分支，不序列化原始 ExitCode：`OK` 时 success `result` 精确只有由 `pytest_report_teststatus` 得到的非空 category 计数组成的 `summary`，其他 ExitCode 时 failure `error` 只表达对应失败类别，不携带 `result` 或 partial summary；Runtime 把完整 pytest terminal report 写向 stderr，CLI 捕获后把同一净化文本同时写入 Operation log 并转发终端，不复制进 `test_pack` Runtime Response，也不由 CLI 解析报告内容。失败 node ID 只属于这份 terminal report，不在私有 Runtime Response 与公开 Response 中重复。CLI 还为 `test_pack` 分配最终 JUnit XML 路径，并经 Runtime 启动控制面提供而不写进 request；Runtime 只把它交给 pytest `--junitxml`，pytest 直接写该随机路径。KAT 信任 pytest reporter，不解析 XML，也不在 Runtime Response 增加报告状态；Runtime 退出后，CLI 只检查该精确路径是否为普通文件。PACK Test Report 是独立的逐测试机器 artifact，不复制进 `test_pack` Runtime Response，也不通过临时发布、目录扫描或恢复协议成为系统状态。该边界只交换控制信息、未解释的 Workflow arguments、Resolved Dataset、Run Output 引用与结果摘要；Parquet 是跨进程数据面，不传输 DataFrame、Logical Plan 或内存 Arrow buffer。

## Pack Authoring API

KAT 自己面向 PACK 作者的 Python authoring surface，顶层 import namespace 为 `kat`。模块级 `kat` 直接导出只提供 Workflow 装饰器、Context 和领域类型；公共 KAT Trace Library 使用明确子包 `kat.trace`，保留的动态子包 `kat.pack` 则暴露当前 Runtime 选中的 PACK 生产代码，其中只有 `kat.pack.workflows.*` 与 `kat.pack.helpers.*`，不表示 manifest 或 PACK object。首版 PACK authoring 以 KAT Skill 驱动 AI 生成源码，再由 `kat inspect --pack` 与 `kat test` 校验真实 Interface 和执行行为；不提供外部编辑器发现、独立 type-checker 安装或动态 `kat.pack` 静态解析界面。公共 API 仍保留普通 Python 内联类型标注，作为源码级约束供 AI、人和运行时代码阅读，但私有 Workflow Host wheel 不携带没有实际消费者的 PEP 561 `py.typed` marker，也不建立 stub package、独立版本或类型检查交付面。PACK 集成测试的 Workflow execution 能力只由 KAT pytest plugin 注入的 `kat_run` fixture 提供，不设置 `kat.testing.run(...)` 等模块级平行入口。所有受 Execution Lease 约束的生产 Workflow execution 能力只通过显式 Workflow Context 提供，也不暴露底层 SessionContext。PACK 还可以直接使用 KAT 原子包固定版本的 PyArrow 与 DataFusion DataFrame API，不由 `kat` 重复包装第三方数据与算子类型。
_Avoid_: Python SDK、Pack API

## KAT Trace Library

随 KAT Skill 原子发布、以 `kat.trace` 向所有 PACK 平等提供的公共 Trace 分析库。它在 Datasource 的稳定 Trace facts 与薄 Query Engine 之上承载经多个真实消费者和真实 Trace 回归验证的可复用语义；不承载来源解码、具体用户问题或尚在单个 PACK 内孵化的候选算法。
_Avoid_: `kat.stdlib`、common PACK、`kat-kernel` 私有公共库

## DataFusion DataFrame API

KAT 原子包随 Bundled Python Host 提供给 PACK 的公开关系计算 Interface，包括 DataFusion `DataFrame`、`Expr` 和官方 functions。`ctx.sql(...)` 与 `ctx.from_arrow(...)` 返回该 DataFrame，`ctx.convert_clock(...)` 返回由同一个执行面拥有的 DataFusion `Expr`；PACK 可继续使用官方算子派生数据并将 DataFrame 作为 Workflow 输出。KAT 不提供 `kat.DataFrame`、`kat.col()` 或第二套表达式系统。DataFusion `SessionContext`、DataFusion catalog 与表注册、SQL options 和执行生命周期不属于该 Interface，PACK 自建 SessionContext 是不受支持的用法。

## Data Import

由一个显式选定的 Datasource type 将其强类型输入转换并写入 KAT Dataset 的用户操作；没有指定目标时创建新 Dataset，明确指定已有 Dataset 时必须要求覆盖并整体替换其内容。每种 Data Import 的成功 `result` 都以 `path` 返回最终 Dataset 的 canonical 绝对 Unicode 路径；Datasource 确有额外、且 Skill 完成当前任务必须知道的短命事实时，再在同一 result 中增加具名字段。Import 不重复 Dataset tables 或 Schema，Skill 随后把该 path 交给 `kat inspect --dataset`，继续以 Dataset Storage inspection 作为唯一结构视图。第一版不支持多个 Datasource 共同生成一个 Dataset，也不支持扩展、追加或部分覆盖。
_Avoid_: materialize、隐式 default Dataset、Dataset extension

## Datasource type

KAT CLI 中一种具有固定名称和强类型导入参数的 Datasource 变体。预发布阶段包含作为长期主线的 Hitrace，以及仅用于内部验证 KAT 机制链路、从首次交付即明确标为 `Deprecated` 的过渡性 Trace Streamer；两者随预发布 KAT 二进制交付，Data Import 显式选择其一，External PACK 不能注册新类型或 CLI 参数。第一次正式发布前必须删除全部 Deprecated Datasource 与命令，不提供兼容期或迁移入口。

## Hitrace Datasource

把原始 Hitrace 文件解码并规范化为 KAT Dataset 的长期 Datasource type。一个 Hitrace 文件可以同时携带多个 clock domain、插件 envelope 观察时间和跨时钟快照；Dataset 只发布当前分析任务具有完整语义和关系的规范化事实，不充当原始文件的逐字段镜像。Datasource 为每个已支持事件保留由 `clock_domain` 与 `clock_value` 共同表达的来源时间事实，不预造一套公共时间坐标。Envelope 时间不是 event time；第一版只在已知插件协议明确二者关系时用于导入一致性校验和诊断，不发布无事件关联的 observation table，也不把它复制进事件表。Datasource 不改写 `.htrace` 内容；用户把原文件保存在覆盖目标之外时，未进入 Dataset 的协议字段仍可由未来能力重新导入。Source 若位于用户显式授权整体清除的 Dataset 目标内，仍会随目录删除。ftrace/Hitrace 的实际 `trace_clock` 是整份输入文件的会话级事实：Datasource 扫描所有非空 `FtraceCpuStatsMsg.trace_clock`，一致重复允许，所有值规范化后必须唯一且受支持，并统一解释包括报告之前在内的全部 ftrace/Hitrace 事件；不按 packet、报告位置或 CPU 分段，也不采用 last-wins。`local` 仍按每条事件的 CPU 形成具体 domain。有这类事件却没有任何有效报告，或报告未知、冲突时，整个 Data Import 失败；文件没有这类事件时不要求报告存在。多个已确认 domain 可以共存；同一 trace segment 的初始 clock snapshot 是该 segment 内跨 domain 换算的有效锚点，KAT 不要求周期校准，也不检测或修正采集期间的映射变化。来源 domain 无法确定、读数编码非法、解释读数所需的 domain 定义缺失，或者已支持内容损坏时，整个 Data Import 失败。合法但未知的 plugin 或 section type 不使导入失败；Hitrace import 成功 `result` 始终包含最终 Dataset 的 canonical `path`，以及去重、稳定排序的 `unsupported_plugins` 字符串数组和 `unsupported_section_types` 整数数组，后两项没有内容时也返回空数组。出现次数、文件位置和技术细节只进入 Operation log；这些短命覆盖事实不写入 Dataset、catalog 或其他 metadata，KAT 也不增加通用 warnings 框架或推断它们是否与用户问题相关。

## sched_switch Source table

Hitrace 为首个长期内核分析闭环发布的直接事件表。每行固定包含非空的 `clock_domain: Utf8`、`clock_value: UInt64`、`cpu: UInt32`、`cpu_switch_sequence: UInt64`、`previous_thread_id: Int32`、`previous_thread_name: Utf8`、`next_thread_id: Int32` 与 `next_thread_name: Utf8`。`cpu_switch_sequence` 是 Importer 按每个 CPU 的输入事件来源顺序从零连续分配的显式序号；它在相同 `clock_value` 下仍定义稳定相邻关系，不能从 Parquet 行序或查询排序稳定性推断。Importer 验证每个 CPU 的时间不倒退及相邻 switch 的 thread ID 连续性。固定 OpenHarmony revision 在清空 kernel trace buffer 前发送 `TRACE_START`，采集停止后发送 `TRACE_END`；因此只用覆盖全部事件 CPU 的唯一完整 `TRACE_END` 统计和每页 `overwrite` 判定本次采集完整性，不相减或检查 `TRACE_START` counter。结束统计的 `overrun`、`commit_overrun`、`dropped_events` 或任一页 `overwrite` 非零时整个 Import 失败，不发布 best-effort 表或统计表。表只在实际解码到至少一行时存在，不为已知 Schema 创建零行占位。

## Trace fact

Hitrace Datasource 从原始 trace 记录直接解码或跨记录规范化得到、供多个 PACK 复用的 Dataset 事实。它仍属于 source data；只有实际产生至少一条事实时对应表才存在，已知 Schema 的零行占位不属于 Dataset 表。

## UnifiedClock

由 `ClockDomain` 与 `ClockValue` 组成的不可变值，表示某个底层时钟域上的一个具体时间读数。统一的是值结构和受支持的时间操作，不是所有来源读数的计量单位。
_Avoid_: ClockReading、SourceTimestamp、把 `UnifiedClock` 当成 Dataset 唯一时间容器

## Clock value

某个 `ClockDomain` 上的非负 `u64` 原生读数。Datasource Adapter 可以对来源编码做不改变数值语义的严格规范化，例如把合法 `tv_sec/tv_nsec` 无损合成为纳秒整数，但 `ClockValue` 本身不把纳秒写进类型承诺；jiffies 等其他计量方式只有在未来某个 Datasource 取得明确频率证据并新增对应 `clock_type` 后才可准入。`ClockValue` 本身没有完整时间语义，必须与 `ClockDomain` 一起解释；Dataset 事件表以 `clock_value` 表达它，列名不承诺单位。

## Clock domain

Dataset 内一个具体时钟坐标的稳定身份，例如 `boottime`、`monotonic`、`monotonic_raw`、`realtime` 或 `ftrace_local_cpu_3`；它是 `UnifiedClock` 的一个成员，由 Datasource 根据采集证据确认并命名。名称完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，是对应 Datasource 的可依赖表契约；Datasource 不直接沿用含糊的来源缩写，例如把 Hitrace 的 `boot` 规范化为 `boottime`。Dataset 已经提供设备、采集和启动的实例边界，因此名称不重复编码 UUID、设备、采集时间、启动 ID 或频率；只有一个 Dataset 内确实存在多个同类坐标时才加入必要 scope，例如 CPU ID。每个 domain 的定义还拥有解释 `ClockValue` 所需的时钟类型与固定整数频率，并在 Dataset 中定义一次，不逐行重复这些参数；跨 domain 对齐证据仍属于 `clock_snapshot`。`clock_type` 只是语义分类，不能作为 target alias。KAT 不做大小写转换、模糊匹配或“唯一同类型时自动选择”；目标不存在时列出当前可用 domain 后失败。相同名称出现在不同 Dataset 中不表示同一时钟实例。

_Avoid_: Source clock、Event clock

## Clock domain table

Datasource 在任何 Dataset facts 引用 `clock_domain` 时同时生成的普通 `clock_domain` Source table。它以 `tables/clock_domain.parquet` 进入既有 Dataset 文件树，每个被引用 domain 恰好有一条定义；Schema 固定为非空 `clock_domain: Utf8`、`clock_type: Utf8` 与 `ticks_per_second: UInt64`，其中 `clock_domain` 是 Dataset 内唯一的具体时钟身份，`ticks_per_second` 必须大于零。第一版 `clock_type` 精确封闭为 `boottime`、`monotonic`、`monotonic_coarse`、`monotonic_raw`、`realtime`、`realtime_coarse`、`ftrace_global`、`ftrace_local`，当前全部以 `1_000_000_000` ticks per second 编码；不接受 `jiffies`、`unknown`、`other` 或 `custom`。其中只有 `realtime` 与 `realtime_coarse` 声明 Unix epoch 墙上时间语义，后者只降低精度；其他类型不论频率是否相同都不表示 UTC。第一版不增加 `unit`、scope、`origin`、`is_unix_epoch`、monotonicity、description、offset、nullable 扩展字段或 JSON 逃生口；不能用当前封闭类型和固定整数 ticks per second 完整解释的来源时钟不准入。该表不增加根级 JSON、catalog、manifest 或 Parquet field metadata 协议。KAT 时钟操作按需把它作为平台证据内部读取，而不在 Workflow execution plane 中隐式注册给 PACK；PACK 直接查询该表时仍须把 `clock_domain` 写入 Required tables。

## Clock snapshot

Hitrace 文件头或 ftrace `clocks_detail` 在同一采集附近对多个 clock domain 顺序取样形成的同步记录，其中每个读数都由一个 `UnifiedClock` 值表达，并由 Hitrace Datasource 保存在普通 `clock_snapshot` Source table 中。表的第一版 Schema 固定为非空 `snapshot_id: UInt64`、`clock_domain: Utf8` 与 `clock_value: UInt64`，`(snapshot_id, clock_domain)` 必须唯一。Snapshot group 只沿用来源容器边界：`.htrace` 文件头中的六个 clock reading 共同构成一组；每个独立 `TracePluginResult` 中非空的整个 `clocks_detail` 列表各构成一组，空列表不产生 group。Datasource 按文件中 group 的出现顺序从零分配 snapshot ID，正常文件的 header 因而是 ID 0；不跨 payload 合并、不把一个列表逐 reading 拆组，也不根据时钟数值或时间距离推断。一个 group 内的来源 clock 名规范化后出现重复 domain，Data Import 直接失败。第一版只使用 `snapshot_id = 0` 作为全 Dataset baseline，来源与目标 domain 必须同时且唯一地出现在该 group；后续 snapshot 只保留，不参与换算，也不因显示 offset 变化而使操作失败。KAT 不拼接不同 group 做多跳转换；相同具体 domain 的转换恒等且不需要 snapshot。Baseline 关系沿用到整个当前 trace segment，但 KAT 不承诺其物理 offset 在采集期间保持稳定，也不检测 suspend、NTP 或手动校时造成的变化。

## Clock conversion

第一版准入的八种 `clock_type` 全部是每秒十亿 tick，因此 KAT 只使用 `snapshot_id = 0` 中来源与目标 domain 的基准读数平移差值。来源值不小于来源基准时，结果是目标基准加二者之差；否则是目标基准减二者之差。实现使用 PyArrow 批量 checked integer kernels，不经过浮点数或 Python 逐行对象；相同具体 domain 直接返回原值。缺少定义或基准读数、频率不是首版固定值、结果小于零或超过 `u64` 时整个操作失败，不返回原值、NULL 或部分结果。第一版不实现异频缩放、`u128` 乘除或舍入；以后准入不同频率的真实时钟时再单独设计和验证该能力。

来源 `clock_domain` 与 `clock_value` 同时为 NULL 时表示外连接等关系运算中不存在可选的 `UnifiedClock`，换算结果传播为 NULL；只有一个为 NULL 则破坏了值对完整性，使整个查询失败。两者都非 NULL 时采用上述严格换算，未知 domain、证据缺失或越界仍使整个查询失败。全空传播不是 best effort，也不把非法非空事实降级为 NULL。`target_domain` 必须是非空固定字符串，SQL 的 NULL 或 Python 的 `None` 在计划或构造边界直接拒绝。

两个来源 Expr 的 Arrow 类型必须精确为 `Utf8 clock_domain` 与 `UInt64 clock_value`；KAT 不把来源 `LargeUtf8`、`Utf8View`、有符号整数、Decimal 或其他可转换类型隐式提升为时钟值，也不为 SQL 与 Python 设置不同 coercion。PACK 确有需要时先使用 DataFusion 的严格显式 cast，负数、越界或非法值在 cast 或计划边界失败；换算诊断显示实际类型与期望类型。SQL target 的公共契约只是普通字符串字面量，Python target 是普通 `str`；Bundled DataFusion 如何物理表示该 literal 属于引擎内部，不构成来源 Expr 的隐式转换或 KAT 类型承诺。

Runtime 把同一个向量化换算实现暴露为 SQL scalar UDF `kat_convert_clock(clock_domain, clock_value, target_domain)` 和 Python `ctx.convert_clock(clock_domain_expr, clock_value_expr, *, target_domain: str)`。SQL UDF 同时注册在 Workflow 与 Output Query 执行面；SQL 的第三个参数必须是字符串字面量，Python 的 `target_domain` 必须是普通固定字符串。两个入口都返回目标 domain 下的 `UInt64 ClockValue`，不返回 Struct 或重复的目标 domain 列。Python 方法只构造调用同一已注册 UDF 的 DataFusion `Expr`，不形成第二份换算实现，也不公开通用 `ctx.udf(name)`。首版唯一实现是 Workflow Runtime 私有的 `stable` Python/PyArrow batch UDF：输入输出保持 Arrow array，计算只复用 PyArrow kernels，不使用 `.as_py()`、Python per-row loop、PyO3、FFI capsule 或 KAT 自建 native wheel。真实性能证据要求下沉时替换这一个私有实现，不改变 SQL 或 Python Interface，也不提前保留第二套 port。调用者需要发布该值时使用 `boottime_clock_value` 之类的自说明列名。用户不能传入 snapshot、频率或 Dataset 路径。

普通用户不选择 `target_domain`，Skill 在执行 `kat run` 前也不根据当前 Dataset 动态发明时钟策略；Workflow 作者按业务语义和 Datasource 表契约在代码中明确目标，并用 Test Dataset 覆盖该前提。第一版不增加时钟域 discovery 命令、Context 方法或 `kat inspect --dataset` 摘要：仅列出 domain 或“可转换”布尔值无法证明具体来源表达式拥有完整 baseline，完整推导又会引入 clock graph 与血缘分析。临时 Output Query 确需查看当前 Dataset 的定义时，直接查询普通 `dataset.clock_domain`；实际换算缺少目标或证据时，诊断列出可用 domain 和具体缺项后失败。只有真实、高频的 `kat run` 前动态选择任务出现后，才设计由 Runtime 拥有的完整能力检查。

时钟换算只改变 `ClockValue` 所在的 domain，不自动把结果提升为 Wall-clock timestamp。第一版只有目标 domain 的 `clock_type` 为 `realtime` 或 `realtime_coarse` 时，PACK 才可以把换算结果交给 DataFusion 的严格 Arrow 类型转换，得到 `Timestamp(ns, UTC)`；`realtime_coarse` 只表示较低精度，不改变 Unix epoch。转换越界或类型不合法时整个查询失败，不使用 `try_cast` 降级为 NULL。KAT 不增加 wall-clock UDF 或平行的 Context 方法。其他 domain 即使同样以纳秒计量，也不因此获得 UTC 语义，KAT 不要求它们能够转成 UTC。

跨 domain 的比较、排序、Join 或相减必须先把两边显式换算到同一个 target domain，再使用 DataFusion 原生运算符；第一版不增加 `kat_clock_compare`、`kat_clock_join`、`kat_clock_diff` 或平行算子。物理 `clock_value` 是普通 Arrow `UInt64`，KAT 不根据列名猜测数值意图，也不以自定义类型、SQL parser 或不完整的 Logical Plan 血缘规则拦截裸整数运算。未换算的运算仍可能被 DataFusion 接受，但其时间语义由 PACK 负责，KAT 不为它背书。

普通执行面创建和未使用时钟换算的 Workflow 或 Output Query 不读取或要求 `clock_domain`、`clock_snapshot`。第一次实际执行换算时，Runtime 才从该执行面可用的 Resolved Dataset 按需读取并严格验证 domain 定义，在进程内构建一次 Resolver 并复用；进程结束即丢弃，不建立磁盘缓存、manifest 或跨进程状态。Workflow execution 使用本次请求可选携带的 Dataset；`query_run` 只使用查询当下从 Run 可选 Dataset reference 重新解析出的当前 Dataset。Output Query 不分析 UDF 参数来自历史 `output.*`、当前 `dataset.*`、CTE 还是字面量，也不保存旧时钟证据或 Dataset revision；路径已被覆盖时仍使用当前证据，由用户承担历史 Output 与当前 Dataset 不一致的后果。没有提供 Dataset、Dataset 当前不可用或缺少证据时，实际执行换算失败，但未执行换算的 Workflow 和纯 `output.*` 查询仍可运行。即使来源与目标相同，也必须有合法 domain 定义；恒等换算不要求 snapshot。实际出现跨 domain 的非空输入时才要求双方同时存在于 `snapshot_id = 0`，缺表或证据不完整使该操作失败。Workflow 内部读取不扩大 Table Grant，PACK 直接查询这两张表仍须声明 Required tables。

## Duration

两个兼容时间点之间的纳秒精度非负经过时长，不携带时区或 clock origin。第一版在 Pack Authoring API 中以不可变的 `kat.Duration` 表达，PACK 使用 `kat.Duration("5ms")` 形式的严格文本构造器创建它；在 Dataset 和 SQL 中以有符号 `Int64` 纳秒承载，并用 `duration_ns` 后缀显式标明语义与单位。它作为 Query Result 的 Arrow `Int64` 标量进入 Skill-facing JSON 时使用十进制字符串，列类型仍明确为 `int64`；有符号物理表示和传输形式都不扩大其非负语义。

从事件时钟派生 Duration 时，PACK 先把起点与终点换算到同一个 `ticks_per_second = 1_000_000_000` 的 target domain，再用 DataFusion 原生表达式计算差值，并显式保证业务上的终点不早于起点、结果不超过 `Int64`。只有满足这些条件的列才命名为 `duration_ns`。KAT 不增加 duration/diff UDF，不自动取绝对值、交换起终点或把负差降级为 NULL；没有可换算到的纳秒 domain 时，第一版不能从该 Dataset 产生 KAT Duration，但仍可保留和分析原始 tick。`kat.Duration` 只作为 Workflow 输入的 Python value object，不是 DataFrame Expr 类型；普通时间相减不会自动提升为 Duration。

## Wall-clock timestamp

带有明确 UTC offset、能够定位到公历时间线的绝对时间。第一版在 Pack Authoring API 中以不可变的 `kat.WallClockTimestamp` 表达，PACK 使用 `kat.WallClockTimestamp("2026-07-14T08:30:00Z")` 形式的严格文本构造器创建它；在 Skill 与 JSON 输入边界只接受带 `Z` 或显式 offset、最多九位小数秒的 RFC 3339 字符串，并把同一 instant 规范化为 UTC。规范输出始终使用 `Z`，最多保留九位小数秒并删除尾零，整秒不输出小数部分；Workflow 参数、Run Manifest 与 Query Result 复用同一格式化规则。Dataset 和 SQL 使用 Arrow `Timestamp(ns, UTC)`。它不接受无时区的 Python `datetime` 或普通整数，也不会仅因某个派生列名为 `timestamp_ns` 就自动与其对齐。

事件时钟只有先显式换算到 `clock_type` 为 `realtime` 或 `realtime_coarse` 的目标 domain，才能由 PACK 使用 DataFusion 的严格 Arrow cast 派生这个类型。普通纳秒 domain 不能据此格式化为 UTC；KAT 不提供额外的 `origin`、`is_unix_epoch`、best effort 转换或专用 wall-clock UDF。

## Workflow temporal literal

用户在 Workflow arguments 中提供两个时间领域类型的封闭 CLI 表达。`kat.Duration` 使用不带符号的 `[0-9]+(?:\.[0-9]{1,9})?(ns|us|ms|s|min|h)`；单位必须是单个小写 ASCII 后缀，不能省略。KAT 只接受能够精确换算为有符号 `Int64` 纳秒的非负值，不使用浮点数，不截断或舍入。第一版拒绝裸数字、空格、复合单位、科学计数、大小写或 Unicode 单位别名，也不要求重复类型语义的 `duration:` 前缀。`kat.Duration` 值保留严格构造时的原始合法 literal，供 PACK inspection 展示作者声明的默认值，不引入第二套单位格式化；`kat.WallClockTimestamp` 沿用带明确 offset、最多九位小数秒的 RFC 3339，并在公开输出中复用规范 UTC formatter。Run Manifest 把实际生效的 Duration 规范化为纳秒整数，把 Wall-clock timestamp 规范化为 UTC RFC 3339，不保留用户的等价拼写。

## Trace Streamer Datasource

把 Trace Streamer 分析数据库转换为 KAT Dataset、仅用于预发布内部验证 Data Import 到 Workflow 执行机制的过渡性 Datasource type。它以只读 SQLite 连接确定性读取源库中全部非系统、可查询的表与视图，并把每个 relation 以同名 Dataset table 完整物化；不使用固定表白名单，也不静默跳过 relation 或列。SQLite 只是在 Import 内部读取源库的临时查询引擎，Workflow Runtime 仍只查询统一的 Parquet Dataset，不出现第二套 Workflow 查询 Interface。`kat import trace-streamer` 从首次交付即在 CLI help 与 KAT Skill 中标为 `Deprecated`，不是生产分析入口；它与 Hitrace 各自拥有自己的数据形状，不建立统一模型、稳定表界面或长期兼容承诺，并必须在第一次正式发布前连同 SQLite 依赖一起删除。弃用事实不进入每次 KAT Response 或 stderr；成功 result 精确只有最终 Dataset 的 canonical `path`，实际 tables 与 Schema 继续由后续 Dataset inspection 给出。
_Avoid_: SQLite Datasource

## Dataset

每次由一个 Datasource 通过 Data Import 完整生成、并可在同一路径整体覆盖的本地列式数据集；它是 Workflow Runtime 的事实输入，不包含 Run Outputs 或 Analysis Result。一个 Dataset 就是一个带 `.kat-dataset` 标记和确定性 Parquet 文件树的具体目录，其文件系统 canonical 绝对 Unicode 路径是 KAT 唯一记录的 Dataset 身份；用户可直接选择并命名该目录，KAT 不在其下再生成一层 Dataset 目录，也不维护独立 Dataset ID。
_Avoid_: 隐式 default Dataset、Dataset ID、`catalog.json`

## Dataset table name

Datasource 写入、Dataset Storage、Required tables、Workflow SQL 和 Parquet 文件名共同使用的唯一逻辑表名。它必须完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，例如 `sched_switch`、`native_hook_alloc`、`clock_domain` 或 `cpu_frequency`，并拒绝 Windows 不可移植的设备名 `con`、`prn`、`aux`、`nul`、`com1`–`com9` 和 `lpt1`–`lpt9`。Dataset Storage 用这条规则校验 Datasource 的写入名，并只把文件 stem 已满足同一规则的直接普通 Parquet 文件识别为受管理表；不符合者不是非法 Dataset table name，而是不属于 Dataset。受管理名称直接映射为 `tables/<name>.parquet`；KAT 不转大小写、不转义、不创建别名，也不维护逻辑名与物理名的第二份映射。第一版不额外维护 SQL 关键字黑名单或解析探针；若名称在实际 SQL 位置与 DataFusion 语法冲突，由该次查询正常失败。

## Dataset Storage

`kat-datasource` package 内拥有的 Dataset 持久化边界，也是 Dataset 受管理条目的唯一写入者和解释者。它使用 Arrow 与 Parquet 的现成能力创建、整体覆盖、解析并校验 Dataset：根目录中无字段的直接空普通文件 `.kat-dataset` 让读取操作识别该目录是 KAT Dataset，每个 Dataset table name 唯一对应直接普通文件 `tables/<name>.parquet`，Schema 只来自 Parquet metadata。每张表的顶层 Arrow field name 按精确字符串必须唯一；Dataset Storage 在创建 Parquet writer 前校验这一领域约束，并在读取 Parquet metadata 后以同一私有约束校验 inspection 与 resolution，重复时点名 table 和 column 后失败。同一 table 的后续 RecordBatch 是否匹配初始 Schema 则直接交给 ArrowWriter，不自建 Schema comparator。该规则不是用户可关闭的选项，也不递归检查嵌套 field、不做大小写归一化或 SQL 关键字限制。用户输入的 Dataset 根可以是平台能够正常解析的 symlink、junction/reparse alias 或挂载路径；Dataset Storage 不分类或拒绝这些形态，而是 canonicalize 后验证 target 目录和其中的直接 marker，并只记录 canonical target path。读取时，`tables` 完全缺席表示零表 Dataset；它一旦存在，就必须是可检查、可读取的直接普通目录，否则保留布局无效，读取失败。有效 `tables/` 中只有名称满足 Dataset table name 规则的直接普通 `*.parquet` 文件是受管理表；没有这类文件仍得到零表 Dataset。目录内其他条目不属于 Dataset，Dataset Storage 不跟随、不解释，也不因其存在而失败。精确满足 `tables/<合法表名>.parquet` 的普通文件是 KAT 保留产物形状；其 Parquet metadata 无效必须使读取失败，避免 KAT 自己生成的表损坏后被悄悄解释为缺席。Datasource type 只通过该边界写入规范化事实，不指定物理路径。显式 Import 目标不存在或是可读取的空目录时直接初始化；已有非空目录无论 marker 是否存在或有效，都必须由 `--overwrite-dataset` 显式授权后才能清除并整体替换。marker 只负责 Dataset 识别，不充当删除许可证；已有目标不是目录时始终失败。KAT 复用成熟递归删除语义，不主动跟随 target 内部的符号链接，也不自建 link、reparse 或 mount scanner。CLI help 必须明确说明输入会解析到 canonical target、其全部内容会永久清除、额外文件不会保留、挂载内容可能受影响，且没有备份、回滚或失败恢复；用户显式授权后自行确认目标并承担目录布局错误的后果。KAT 不检查类型化 source 是否位于目标内，也不复制、暂存或特别保留它。Dataset Storage 不根据 Dataset 表数量或 Parquet 行数推断业务有效性；零张表或零行表是否应由一次 Data Import 产生，属于具体 Datasource。它不执行 SQL、不依赖 DataFusion，也不维护 catalog、Dataset ID 或其他持久索引；它通过面向具体操作的窄内部 Rust Interface 向 KAT CLI 返回已验证的 Dataset 事实，inspection 与未来 resolution 不共用公开结果类型。

`--overwrite-dataset` 的 help 语义文本固定为 `Replace the Dataset at the resolved target path. Permanently deletes all existing contents, including unrecognized files. Linked or mounted paths may affect data outside the path you typed. No backup, rollback, or failure recovery is provided.`。Clap 可以按终端宽度正常排版；测试只检查这四项风险语义，不锁定换行或空格快照。

## Resolved Dataset

Dataset Storage 解析并校验一个 Dataset 的受管理条目后返回的短命内部值。它只有 `path` 与 `tables` 两项信息：`path` 是 Dataset 的 canonical 绝对 Unicode 路径，`tables` 是从可选普通 `tables/` 目录中符合受管理表形状的直接普通文件得到的完整 Dataset table name 到已校验 canonical Parquet path 映射，没有该目录或受管理表时映射为空。被忽略的其他目录内容不进入该值。它不持有打开的文件或目录句柄，不携带 Schema、行数、大小、Datasource、marker、相对路径或 Dataset ID，不持久化为第二份 Dataset 元数据，也不形成用户或 PACK Interface。解析成功时，KAT CLI 把 Resolved Dataset 原样编码进相关 Runtime Request；Workflow Runtime 只逐项消费这些已验证引用并建立相应执行上下文，不扫描目录、拼接表路径或转换 file URI。`run_workflow` 只在用户提供 Dataset 时要求解析成功；`test_pack` 对已声明的 Test Dataset candidate 要求解析成功。`query_run` 面对没有 Dataset reference 的 Run 直接使用 `not_provided` dataset；有 reference 时解析成功编码为 `available`，失败则编码为带记录路径和 cause 的 `unavailable`。

## Dataset inspection

`kat inspect --dataset <dataset-directory>` 由 Dataset Storage 从当前文件树即时生成的只读描述。成功 `result` 直接是 `{"path":"...","tables":[...]}` Dataset object，不增加 `dataset` wrapper；`path` 是 Dataset canonical 绝对 Unicode 路径，`tables` 始终存在并按 table name 排序，空 Dataset 返回空 array。每张表恰好包含 `name` 与 `columns`；`columns` 始终存在、保留 Parquet metadata 中的 Arrow field 顺序，每项恰好包含 `name`、锁定 arrow-rs `DataType::to_string()` 得到的 `type` 与 boolean `nullable`。它不输出行数、大小、每表物理路径、Datasource、marker、独立 Arrow metadata 字段或时钟语义，也不重建递归 Arrow Schema JSON；嵌套类型中由 arrow-rs Display 原生带出的 field 信息仍属于 `type` 字符串，KAT 不自建清洗或第二套类型格式。该操作不启动 Workflow Runtime、不读取表数据，不成为持久 catalog；成功与 failure 都不创建 Operation log、不返回 `log_path`，也不创建 KAT Data Home 或 `logs/`。无目标 `kat inspect` 与 `--pack` 模式不混入这些字段。

首次 inspection 切片中，`kat-datasource` 的唯一 workspace Interface 是 `inspect_dataset(&Path) -> Result<DatasetInspection, DatasetInspectionError>`。`DatasetInspection`、`TableInspection` 与 `ColumnInspection` 以私有字段和只读 getter 暴露 canonical path、排序后的表、Schema 顺序列、由 datasource 侧生成的类型字符串与 nullability，不实现 `Serialize`；`kat-cli` 不依赖 Arrow，而是显式投影自己的 `InspectDatasetResult`。该 PR 不实现尚无调用方的 `ResolvedDataset`、`resolve_dataset()`、writer、storage trait 或 provider registry；生产源码只需 package 的 `src/lib.rs`，CLI 继续把两个简单只读 inspect 模式共置于现有 `lib.rs`。

## Datasource

完成 Data Import 的代码边界。它识别、读取并规范化外部 source，拥有可复用 source facts 的生成、“哪些记录属于重复数据”以及没有事实时应失败、缺席表还是产生零行表的领域语义，并通过 Dataset Storage 写入 Dataset；Dataset 通用能力不从表数量或行数推断业务有效性。它不依赖 DataFusion，不运行 PACK，也不参与 Workflow execution plane。

## `pack.toml`

PACK 必需的最小静态清单，恰好包含根级 `name`、`title`、`description` 和 `owner` 四个必填非空 string，不增加 `[pack]` 包装。CLI 对 `title`、`description` 与 `owner` 使用 Rust `str::trim()` 去掉外层 Unicode whitespace，清理后为空即失败，公开 Interface 使用清理值并保留内部空白与换行；机器身份 `name` 不做文本规范化。PACK identity 只取自 manifest name；目录 basename 只是位置，不参与身份，也不要求与 name 相同。PACK discovery 不导入 Python 源码。任何未知 key 或 TOML table 都直接失败，以暴露拼写和错误假设。Workflow name 与函数约束由各入口旁的装饰器声明，不在清单中重复维护 Workflow 列表；第一版也不声明 manifest/schema version、PACK version、入口路径、dependencies 或通用扩展区。

## PACK source layout

单个 PACK 目录使用固定、浅层的代码视图：可选 `workflows/` 保存 Workflow 入口，可选 `helpers/` 保存 PACK-local 普通 Python，可选 `tests/` 保存 `pytest` 测试及按需存在的 `datasets/` Test Dataset，并可在 PACK 根目录和测试树的适用层级放置标准 `conftest.py`。Runtime 把 `workflows/` 与 `helpers/` 作为当前 `kat.pack` 的生产代码加载；pytest 原生拥有物理 `tests/`，KAT 不为测试模块建立 Python identity。Runtime 按可移植的相对路径顺序递归扫描 `workflows/` 中的普通 `.py`；目录缺席或没有 `.py` 得到零 Workflow，非 Python 文件忽略。入口相对路径的每个目录名与 `.py` stem 都原样成为 `kat.pack.workflows.*` 下的 Python module segment，必须满足 Python 3.14 的 `str.isidentifier()` 且不是 `keyword.iskeyword()` 判定的关键字；KAT 不附加 ASCII 或 snake_case 规则，不清洗、散列或建立别名，非法时点名路径并使目标 PACK 的生产 Interface 加载失败。每个 `.py` 都必须恰好声明一个本 module 定义的 `@kat.workflow(...)`，零个或多个均失败；入口不能 import 另一入口，import 的 helper 也不能通过副作用注册 Workflow，共享实现只放 `helpers/`。`workflows/` 下的 `__init__.py` 明确拒绝而不是静默忽略；`helpers/` 不要求该文件，`tests/**/__init__.py` 则完全服从 pytest。目录无法读取、任一入口导入、注册来源或声明失败时，目标 PACK 的生产 Interface 加载失败且不返回部分 Workflow。PACK 根目录中的其他目录，包括不再属于现行 Interface 的 `capabilities/`，不扫描也不因存在而失败；只有 `pack.toml` 中的 `dependencies` 等未知字段会因进入 KAT 自己的清单边界而失败。KAT 不扫描单个 PACK 目录下的所有 Python，不支持特殊 `pack.py` 初始化入口，也不在 `pack.toml` 中开放自定义扫描路径。

`kat.pack` 生产导入边界在导入任何 Workflow 入口前检查扫描结果的 Python module identity。若同时存在 `workflows/cpu.py` 与 `workflows/cpu/.../*.py` 这类要求同一 segment 兼任普通 module 和 package 的结构，目标 PACK 的生产 Interface 加载失败，诊断同时指出冲突文件与目录中的代表入口。KAT 不按扫描或导入顺序选择一方，也不生成合成模块名；只有同名目录内确实存在被扫描的 Workflow `.py` 后代时才形成冲突，普通资源目录不受限制。

`helpers/__init__.py` 可选而不是被忽略：缺席时 `kat.pack.helpers` 使用标准 namespace package 语义，存在时作为普通 package initializer，只在首次 import 时按 Python 语义执行一次。KAT 不因目录扫描而主动执行 initializer；它不成为 Workflow 入口，initializer 副作用产生的 Workflow 注册仍因来源非法而失败。`tests/**/__init__.py`、test module、fixture、hook 与 `conftest.py` 的加载语义全部属于 pytest；`workflows/**/__init__.py` 继续拒绝。

## Workflow decorator

PACK Python Workflow 唯一的注册方式，公开调用形状固定为 `kat.workflow(*, name, title, required_tables, parameters=None)`。Decorator 必须带括号且只接受关键字；`name`、非空 `title` 与 `required_tables: list[str]` 始终必填，没有 PACK-visible Dataset 表依赖时也必须显式写 `required_tables=[]`。存在除 `ctx` 外的用户输入时必须提供 `parameters: dict[str, str]`，其 key 与全部用户参数精确一致且 value 是非空说明；无用户输入时可以省略。Decorator 对 `title` 和参数说明 value 使用 Python `str.strip()` 去掉外层 whitespace，清理后为空即失败，公开 Interface 使用清理值并保留内部空白与换行；机器名称、Required tables、choices 与 default 不做文本规范化。Decorator 应用时立即复制这些容器并形成 Workflow 不能再修改的私有规范约束；此时尚无本次调用，不能检查 Dataset 是否提供。Python Runtime 选定 Workflow 后才在创建 Table Grant 前使用该约束校验可选 Dataset，Rust CLI 不解释 Required tables，也不条件化 `--dataset`。PACK 不使用裸装饰器、位置实参、tuple 形式的 Required tables、`description=`、未知扩展项、`Annotated`、`kat.Param`、结构化 docstring、Click、Typer 或其他 CLI 框架声明 Workflow Interface。类型、default、required 与 choices 只从函数签名和类型标注推断；Workflow description 的唯一来源是紧邻函数定义的普通 Python docstring，KAT 使用 `inspect.cleandoc()` 后再 `strip()` 并拒绝清理后的空文本，读取全文但不解析 `Args:` 等结构化方言。参数展示顺序服从函数签名。

## Workflow parameter constraints

Workflow callable 只能是当前入口 module 顶层以普通同步 `def` 定义的 Python function，不接受 nested function、method、lambda、callable object、`async def`、generator 或 async generator。第一个参数必须精确命名为 `ctx`，没有默认值，解析后的标注精确为 `kat.Context`，并且是 positional-or-keyword；Runtime 只把它作为第一个位置参数传入。其余参数才是 Workflow 用户输入，可以是 positional-or-keyword 或 keyword-only，Runtime 始终按名称传入；positional-only、`*args` 与 `**kwargs` 直接拒绝。输入标注服从 Bundled Python 3.14 的延迟求值语义，Compiler 使用标准库 `annotationlib` 在函数定义作用域内只解析 `ctx` 和用户输入；可解析的 forward reference 与直接类型标注等价。任一输入无法求值或最终不属于 KAT 封闭类型集时，诊断点名该参数并使目标 PACK 的生产 Interface 加载失败。KAT 不解析源码文本、不建立自定义标注语法，也不求值或校验 return annotation；不可解析的 return forward reference 不影响 Interface。decorator 只用 `parameters` 补充每个输入的自然语言说明，不重复声明类型、default、required 或 choices。Workflow Runtime 从两者生成同一份 inspect 描述与 Python 参数解析器，PACK 不能另行提供自己的 CLI parser。Compiler 按 Click 文档的常规形式给精确 Python 参数名添加 `--` 并把 `_` 替换为 `-`；bool 使用 Click 原生 `--name/--no-name` pair，省略时使用函数默认值。名称接受、冲突与 warning/failure 语义只属于原子包锁定的 Click，KAT 不增加自己的字符正则、`no_` 禁止、保留名称或冲突校验。私有 Click Command 显式关闭自动 help option，因此 `help` 不是 Workflow 参数保留名；`--` 后的 `--help` 只有在实际参数生成该 option 时才是输入，否则由 Click 按未知 option 失败。Compiler 把 Click Command 实际使用的正向 option 和 bool 反向 option 直接发布到 PACK inspection，Skill 不重复映射。第一版 Workflow argv 不提供位置参数、短选项、手工别名或 bool 文本值。Runtime 解析成功后始终调用 `workflow(ctx, **effective_inputs)`。

## Workflow input inference

Workflow 输入参数默认从 Python 函数签名推断。无默认值的参数是必填输入，有默认值的参数是可选输入，且所有用户参数都必须有受支持的 type hint。第一版封闭类型集合只有 `str`、有符号 64 位范围内的 `int`、有限 `float`、有默认值的 `bool`、`kat.Duration`、`kat.WallClockTimestamp`，以及只包含字符串成员的 `Literal`。bool 按 Click 惯例生成 `--name/--no-name` 正反 flag，不接受 `--name true` 或 `--name false`；省略时使用函数默认值。函数签名中的 raw default 不做 KAT 精确类型预检：Compiler 把 annotation 编译成 Click `Option` 与对应 `ParamType`，默认值和显式 argv 再共同经过该类型的转换与校验；因此 `ratio: float = 1` 得到 `1.0`，合法的时间文本默认值也由相应 temporal `ParamType` 构造成领域值。`T | None` 只允许包裹上述非 bool 类型且默认值必须是 `None`；CLI 只能通过省略该选项得到 `None`，没有显式 null token。choices 只来自 `Literal`，成员必须都是字符串且至少一个；Compiler 以 Python 普通字符串顺序去重排序，同一列表同时驱动 inspection 与 Click 精确匹配。默认值仍独立表达，不让 choices 首项承担优先语义。不支持缺失标注、`Any`、其他 Union、Enum、Path、Python `datetime` 或 `timedelta`、容器、dataclass、Pydantic model、PACK 自定义类型或 parser。同一个 Workflow Input Compiler 负责 inspect 与 Workflow execution 参数解析；decorator 的 `parameters` 只补充参数说明，新增、删除或重命名签名参数后未同步该映射会使目标 PACK 的生产 Interface 加载以 missing 或 unknown parameter 直接失败。

## Workflow Input Compiler

Workflow Runtime 内根据实际函数签名和 KAT Workflow decorator 编译 Workflow 输入约束的私有 Module。Decorator 应用时立即复制、校验并规范化 name、title、Required tables 与参数说明，Compiler 再校验 `parameters` 与所有非 `ctx` 签名参数精确对应并以签名顺序合并说明；作者随后修改传入的 list/dict 或执行 Workflow 都不能改变这份私有约束。规范化的 KAT 输入约束是唯一真相源，共同驱动 PACK inspection、生产 `kat run` 与 PACK test `kat_run` 的 Workflow execution；Python Runtime 还以其中的 Required tables 在创建 Table Grant 前校验可选 Dataset，CLI 不复制这项 Workflow 语义。Compiler 同时生成 PACK inspection 中语言无关的封闭 `type`、真实 `option`、bool `negative_option`、required/default 与 choices；字符串 Literal 的 choices 以普通 Python `sorted(str)` 去重规范化，不保存作者顺序。Compiler 使 Skill 只消费结果而不重建 Python 到 CLI 的映射，并根据同一约束程序化构造关闭自动 help option 的 Click `Command` 与 `Option`，从原始 Workflow arguments 得到 effective Workflow input values，但不调用 Workflow。KAT 负责把受支持的 annotation 与领域边界编译成 Click 参数；Click 统一负责签名默认值和 argv 的取值、转换、choices、必填校验与解析错误，Compiler 不再维护 raw-default 类型表或数值拓宽规则。整数范围、有限浮点数和时间领域语义都通过 Click `ParamType`/range 表达，使默认值与显式输入经过同一个转换器；inspection 也必须从该转换器取得 effective default，不能直接读取函数签名的 raw object。私有 Command 不启用 env、`default_map` 或 prompt。KAT 仍拥有封闭且不超出 Click 公开能力的类型集合、inspect 格式与 diagnostic，不因 Click 支持更多类型或选项而自动扩大 Workflow Interface。PACK 不导入 Click、不使用 Click decorator、annotation、类型或自定义 parser，Click 对象和异常也不越过 Runtime Interface。

## Workflow module top level

PACK 的 Workflow 入口模块在 Workflow discovery 阶段会被导入，并在同一次 PACK test 会话的多个 `kat_run` 间保持已加载状态。模块顶层只能做 import、不可变常量定义、无状态函数或类型定义，以及当前 module 唯一普通同步 Workflow function 的装饰器声明；不做实际分析、IO、耗时工作，也不保存单次 Workflow execution 的可变状态。import 不能替当前入口贡献另一项注册；所有 Workflow execution 状态必须来自显式 Workflow Context、函数参数或局部变量。

## Workflow name

Workflow 通过完整的 `@kat.workflow(...)` 显式声明、在所属 PACK 内唯一的一级公共身份，匹配 `^[a-z0-9]+(?:-[a-z0-9]+)*$`。PACK name 提供它的业务选择作用域；`kat inspect` 列出的值就是 CLI、PACK test、Run Manifest 和模型调用共同使用的精确名称。Python 文件路径、目录层级与函数名都不参与身份推导，移动源码或重命名函数不会改变 Workflow name；修改显式 name 是有意的破坏性身份变更，第一版不提供 alias 或迁移机制。`workflows/` 中每个普通 `.py` 必须恰好声明一个本文件定义的 Workflow；零个、多个、缺失/非法 name 或 PACK 内重复 name 都直接失败。
_Avoid_: 从文件路径或函数名推导的 Workflow 名、多级 Workflow namespace

## PACK helper modules

PACK 内的 Workflow、测试与 conftest 通过稳定的 `kat.pack.helpers.*` 绝对名称访问 `helpers/` 中的普通 Python 模块；Python 原生相对 import 不另行禁止，但 KAT 文档和示例不依赖文件层级。Workflow Runtime 把所选 PACK directory 作为 `kat.pack` 的唯一搜索位置，不加入全局 `sys.path`。需要 Workflow execution 能力的 helper 必须显式接收 Workflow Context，纯计算 helper 不接收它。普通 helper 是 PACK 实现，不单独声明 Table Grant；它读取的表必须由 Workflow 的 Required tables 覆盖。

## PACK

由一个明确组织或团队拥有并独立维护的一组性能分析策略构成的自包含扩展单位；其边界服从组织所有权与发布责任，而非 Workflow 数量、代码规模或单个分析问题，同一 owner 只有在形成彼此独立的维护与发布责任时才拆成多个 PACK。第一版 PACK 通过 Pack Authoring API 与 KAT Trace Library 使用 KAT 能力，不依赖、导入或调用其他 PACK，既可作为 Bundled PACK 随 KAT 提供，也可作为 External PACK 在私有环境中部署。
_Avoid_: 每个 Workflow 一个 PACK、每个分析问题一个 PACK、无明确所有者的 `common` 或 `utils` PACK

## PACK owner

`pack.toml` 中随 PACK 分发的唯一必填非空组织或团队名称，用于让 inspect 和维护者识别 PACK 的看护责任；每个 PACK 只有一个 owner，但同一 owner 可以拥有多个 PACK，KAT 不校验 owner 唯一性。它是可变的展示元数据，不是 PACK identity、namespace、发布者认证或权限依据；修改 owner 不改变 PACK name。
_Avoid_: Publisher ID、所有权认证

## Bundled PACK

随 KAT Skill 同版本发布的自包含 PACK。它使用与 External PACK 相同的一级 PACK name 模型和无跨 PACK 依赖约束，是预置策略和独立部署 PACK 的可运行范例，只能使用与 External PACK 相同的 Pack Authoring API 和 KAT Trace Library；即使由 KAT 平台维护团队拥有，也不获得 Workflow Runtime 特权。新建的 KAT 自带 PACK 推荐使用 `kat-` 前缀强化品牌，但这只是命名惯例：discovery 不校验或保留该前缀，任何 PACK 都可以使用或不使用它，既有 PACK 被收编进 Skill 时也不要求改名。
Bundled/External 只描述 KAT 内部的发布与目录来源，不是用户需要选择的 PACK kind；CLI、PACK list、PACK object 和成功后的 `DiscoveredPack` 都只呈现 PACK。
_Avoid_: Built-in PACK、System PACK

## Complex PACK demo

一个随预发布 KAT Skill 交付、可被用户正常发现和运行的普通 Bundled PACK，其用途是用复杂真实场景验证公共 PACK Interface、测试与部署链路的表达能力。当前 identity 为 `kat-openharmony-demo`；它在 manifest 中诚实标明 Demo，不形成新的 PACK kind、Runtime 权限或专用 API，也不自动承诺其中算法与结果已经达到正式分析产品的可信度。预发布阶段它照常参与 Skill 的 PACK/Workflow 自动选择，不增加显式运行限制、名称硬编码排除、`automatic` 开关、信任等级或阶段字段；正式发布前通过统一发布清理门决定迁移、晋升或删除。该可信度门、Deprecated 依赖闭包和退场责任构成与同 owner 正式 PACK 不同的维护与发布责任。
_Avoid_: 内部测试夹具、特权 Demo PACK

## kat-kernel PACK

KAT 首个 Bundled PACK，name 为 `kat-kernel`，由内核团队维护，初始 title 为 `Kernel Performance`。它按稳定组织所有权承载内核调度与执行分析，不因首个问题属于调度领域而拆成 `kat-scheduling`，也不因平台维护者与 PACK owner 恰好都是内核团队而获得额外 Runtime 权限。

## kat-openharmony-demo PACK

KAT 预发布阶段的 Complex PACK demo，由 Kernel Team 维护并与正式的 `kat-kernel` 分离。分离依据是 Demo 独立的可信度门、Deprecated Trace Streamer 依赖闭包，以及正式发布前必须独立作出的淘汰、迁移或晋升决定；这些构成独立维护与发布责任，而不是为单个 Workflow 创建 PACK。预发布 Skill 不在自动选择中排除它；它仍是无特权的普通 Bundled PACK，正式发布前与全部 Deprecated 能力一起系统清理。

## External PACK

不与 KAT Skill 同版本发布、由用户或第三方在本地受信任环境中部署的源码与领域资源包。External 是 KAT 内部的部署分类，不是公共 PACK kind；合作伙伴可以让仓库根直接成为 PACK directory，并用可重复的 `--pack-dir <directory>` 将每个精确目录作为本次 PACK discovery 的显式 candidate。参数目录自身必须直接包含 `pack.toml`，KAT 不扫描它的子目录，也不猜测它是单个 PACK 还是 PACK 集合。需要随包交付测试时，`tests/`、`workflows/`、`helpers/` 与 `pack.toml` 位于同一个 PACK 目录并一起版本化；Test Dataset 如有则位于 `tests/datasets/`，不要求为不使用 Dataset 的测试创建该目录；KAT 不增加独立测试包、测试 root、测试 manifest 或 PACK 与测试的配对机制。External PACK 可以在纯运行部署中省略 `tests/`，不影响 PACK discovery、`kat inspect --pack` 或 `kat run`，但显式 `kat test` 必须失败。它必须声明一级 PACK name；若与本次发现到的其他 PACK 重名，整个 discovery 失败，也不能隐式覆盖随 Skill 发布的 PACK。`kat-` 前缀不受限制，使用与否都不改变该 PACK 的发现位置、所有权或运行权限。无目标 `kat inspect` 只发现并列出它；`kat inspect --pack`、`kat run` 与 `kat test` 才在选中后加载它。KAT 平台自身不修改、复制或安装 PACK 源码；受信任的 PACK 或 pytest 代码主动写文件由作者负责，不属于 KAT 的只读或安全保证。PACK authoring flow 只修改用户明确指定的源码位置。它扩展 Dataset 之上的 Workflow 与领域策略，不注册 Datasource type，也不依赖其他 PACK；它只依赖 Python 标准库、Pack Authoring API 和 KAT 原子包公布并固定版本的 bundled libraries，第一版包括用于内存表表达的 PyArrow，以及用于 DataFrame、Expr 与官方 functions 的 DataFusion DataFrame API。它不携带独立解释器、venv 或任意 native wheel，也不在运行时安装依赖。
_Avoid_: 仅内置 PACK

## PACK name

PACK 的一级机器名称，也是 manifest 和 CLI 使用的唯一业务身份。一次成功的 PACK discovery 保证 Discovered PACKs 按 name 唯一。PACK name 只使用小写 ASCII kebab-case，必须匹配 `^[a-z0-9]+(?:-[a-z0-9]+)*$`，并拒绝 `con`、`nul`、`com1` 等 Windows 保留设备名；title 和 description 可以使用 Unicode。PACK name 不包含结构化的发布者或来源层级；`kat-` 只是 KAT 自带 PACK 可选的品牌命名惯例，既不保留也不证明发布者，任何位置的 PACK 都可以使用或省略它。不同 canonical PACK directories 提供相同 name 时直接拒绝。无目标 `kat inspect` 只列出 Discovered PACKs；`kat inspect --pack`、`kat run` 与 `kat test` 从中按精确 PACK name 选中目标，不把源码目录当作另一种身份。移动或重命名 PACK directory 不改变 manifest 声明的 name，再次执行 PACK discovery 后仍以该 name 选中。PACK name 不参与 Python module name 计算；每个短命 Runtime 都把本次唯一选中的 PACK 暴露为相同的 `kat.pack`。
_Avoid_: PACK ID、`<namespace>/<name>`

## 当前 PACK Python 包（`kat.pack`）

Workflow Runtime 为当前进程唯一选中的 PACK 动态挂载的稳定、公开 Python package。顶层 `kat` 来自 Workflow Host wheel 并提供 Pack Authoring API；`kat.pack` 只承载当前 PACK 的 `workflows/` 与 `helpers/`，不是 manifest 或 PACK object，也不包含 `tests/`。`kat.pack.workflows.*` 是 Runtime 标识和加载 Workflow 入口时使用的规范 module identity，不是入口之间的复用接口；PACK 作者、测试与 conftest 只通过 `kat.pack.helpers.*` 共享普通 Python 实现，Workflow 集成测试通过 `kat_run(workflow=...)` 进入生产 Interface 加载边界。PACK name 仍是业务身份，但不编码进 Python namespace。每个短命 Runtime 只挂载一个 PACK，其他 PACK 不进入 KAT 建立的 module search path；引用其他 PACK 时由 Python 按普通导入规则自然失败，KAT 不扫描源码或安装额外 import 拦截器。`kat.pack` 只在 `inspect_pack`、`run_workflow` 与 `test_pack` Runtime 中挂载；`query_run` 不选择或挂载 PACK，裸系统 Python 或裸 pytest 也不属于受支持 Interface。

Runtime 以目标 PACK directory 作为 `kat.pack` 唯一的 `ModuleSpec.submodule_search_locations`，并把已校验入口交给 `importlib.import_module()`；标准 `PathFinder` 与 `SourceFileLoader` 独占源码解码、module/package 查找、initializer 执行、模块属性、缓存与 traceback。KAT 不实现 `read_text + compile + exec`、自定义 `MetaPathFinder`、源码临时复制或 `sys.modules` alias，也不修改全局 `sys.path`；它只拥有生产入口扫描、路径和 module identity 校验以及 Workflow 注册来源约束。pytest 独立拥有物理测试树，测试只通过 `kat.pack.*` 使用生产代码。进程结束即释放该挂载，不建立持久 import registry。
_Avoid_: PACK-name Python namespace、`kat_pack` alias、`kat.pack.tests`

## PACK directory

直接包含 `pack.toml`、可选 `workflows/`、`helpers/` 与 `tests/` 的单个 PACK 文件系统边界，合作伙伴仓库根可以直接成为该目录。无目标 `kat inspect`、`kat inspect --pack`、`kat run` 与 `kat test` 接受的可重复 `--pack-dir <directory>` 每项精确加入一个 PACK directory；该目录必须存在、是可读目录、可 canonicalize，并直接包含有效 `pack.toml`。manifest name 是 PACK identity，可以与目录 basename 不同。CLI 不扫描参数目录的子目录，不自动判断它是单个 PACK 还是 PACK 集合；即使它的直接子目录含有合法 PACK，只要参数目录自身没有根级 manifest 就失败。同一 canonical directory 无论从哪个查找位置发现都只处理一次，重复输入是幂等的。

## Default PACK search directory

PACK discovery 只扫描两个隐式目录：KAT Skill 内的 `assets/packs/` Bundled PACK search directory，以及 KAT Data Home 内的 `packs/` External PACK search directory。每个目录只把直接子目录中存在 `pack.toml` 的条目声明为 PACK candidate；candidate 的 PACK name 来自 manifest，不从子目录名推导。没有 manifest 的兄弟条目忽略，零 candidate 是正常空结果。默认 search directory 尚不存在时视为空且不创建，存在却不是可读目录时失败。第一版没有用户可配置的 PACK search directory，不递归发现更深层 PACK。

## PACK discovery

KAT CLI 只在需要列出或选择 PACK 的操作中执行短命 PACK discovery：无目标 `kat inspect`、`kat inspect --pack`、`kat run` 和 `kat test`。`kat import`、`kat inspect --dataset` 与 `kat query` 不读取 PACK search directories，也不会因无关 PACK 损坏而失败。`kat-cli` package 内的 `pack_discovery` Module 接受调用方使用平台目录能力与当前命令参数构造的 `PackDiscoveryPaths`；该 Module 不成为独立 Cargo package 或部署单元。该内部值恰好包含 `skill_pack_search_directory`、`data_home_pack_search_directory` 与 `additional_pack_directories`，`pack_discovery::discover(paths)` 不解析 OS 默认目录或 CLI，也不接受无标签路径列表、通用 root、优先级或可扩展 source enum。前两个具名位置扫描一级子目录，最后一个列表中的每项自身就是由 `--pack-dir` 增加的精确 PACK directory；字段表达位置和扫描方式，不表达 PACK kind。每次 discovery 都从这三项输入形成完整 candidate 集合，校验并 canonicalize candidate PACK directories、按 canonical directory 去重，再原子校验全部 manifest、统一 PACK name 语法与跨目录重名；同一 canonical directory 即使由多个输入找到也只是一个 candidate，不产生来源或身份冲突。`kat-` 不形成校验分支。Discovery 不因操作只选择一个 PACK 而缩小或跳过候选集合。成功返回 `DiscoveredPacks`，失败返回 `PackDiscoveryError`。任一 additional directory 无效、缺少根级 manifest，或任一已声明 candidate 损坏，都会使整个 discovery 失败且不返回部分结果；零 candidate 是可信空结果。

Discovery 使用确定性的 fail-fast：依次处理 Skill PACK search directory、Data Home PACK search directory，以及保持 CLI 出现顺序的 additional PACK directories。每个 search directory 先完整读取一级 entries；枚举失败立即报告该目录且不使用已经读到的部分，枚举成功后按当前平台的 `PathBuf::Ord` 排序再逐个端到端处理 candidate。遇到第一个使完整 `DiscoveredPacks` 无法成立的错误便立即返回，不继续验证、不聚合 sibling errors，也不返回部分结果；这个顺序只稳定同一平台与文件树上的首条诊断，不建立来源优先级或覆盖规则。`PackDiscoveryError` 使用 `thiserror` typed variants，保留相关路径、PACK name 及真实 I/O/TOML source chain；重名同时保留两个目录。Module Interface 不使用 `Vec<Error>`、`anyhow` 或 string details bag，第一条 Module PR 也不引入 `miette`、serde diagnostic 或公共错误码。后续 operation handler 在本地把它包装为 operation-specific miette Diagnostic 并决定用户语义，共享 adapter 只从该 Diagnostic 及真实 source chain 机械投影稀疏 KAT diagnostic；公开 diagnostic 只描述无效目录、清单、name 语法或重名等可操作事实，不要求用户理解内部部署位置。这里的原子只表示成功返回全部、失败不返回部分，不表示收集全部错误。
_Avoid_: PACK indexing、注册、catalog 构建、`PackLocations`、无标签 `Vec<PathBuf>`

## Discovered PACKs

一次成功 PACK discovery 得到的完整、不可变、静态可信 PACK 集合。`DiscoveredPacks` 是 `kat-cli` crate-private 类型，封装全部表示，只通过 crate 内的 `iter()` 按 PACK name 稳定排序遍历，并通过 `get(name: &str) -> Option<&DiscoveredPack>` 做精确选择；unknown PACK 的用户诊断属于 CLI。每个私有字段的 crate-private `DiscoveredPack` 恰好提供 `name()`、`title()`、`description()`、`owner()` 与 `directory()` 只读访问，其中 directory 保证是已校验的 canonical PACK directory。它不增加 `PackManifest` 外层、公开 `PackName` newtype 或 Bundled/External 来源字段，也不暴露内部使用 `BTreeMap`、`HashMap` 或其他集合实现。`DiscoveredPack` 与 `DiscoveredPacks` 不实现 `Serialize`；后续 CLI 必须显式把前四个字段投影为无目标 `kat inspect` 的 `{"packs":[...]}`，不能意外发布 directory。`Discovered` 只表示 PACK directory 与 manifest 已通过校验，不表示 Workflow 已成功导入、PACK 可以运行或 `tests/` 存在。该集合不导入 Python、不解析 PACK 依赖，命令结束后丢弃，也不是持久 registry、catalog、公开 index 或公共 Rust API。
_Avoid_: PACK index、Pack Indexer、Available PACKs

## Workflow

PACK 中的一个可运行分析入口。它表达一次具体分析任务的输入、入口函数和 Run Outputs，例如线程 CPU 时间分析或调度等待分析。

## thread-cpu-time Workflow

`kat-kernel` 的首个 Workflow，回答“哪些非空闲线程占用了最多 CPU 时间，主要运行在哪些 CPU 上”。它没有用户参数，只声明 `required_tables=["sched_switch"]`，用 DataFusion window functions 在每个 `(clock_domain, cpu)` 内按 `cpu_switch_sequence` 计算相邻 switch 的完整可观测区间，排除 idle 后按 thread ID、观测名称与 CPU 聚合。唯一 Output `thread_cpu_time_by_cpu` 包含 `thread_id`、`thread_name`、`cpu` 与 `observed_cpu_time_ns`；`observed_` 明确排除每个 CPU 首条 switch 之前和末条 switch 之后无法闭合的边界时间。Skill 通过有界查询从这张表求跨 CPU 排名，不重复持久化总量表，也不增加 top、窗口、include-idle 或百分比参数。

## first-frame-scheduling-dependencies Workflow

`kat-openharmony-demo` 唯一公开的 Workflow，回答“指定进程按时间最早的已完成实际帧，存在哪些可观测的线程状态与调度依赖”。它只承诺调度观测和依据可用 wakeup 事实得到的 blocker 归属，不声称已经计算完整因果关系或关键路径；`critical_path` 名称保留给未来经真实 Trace 回归验证后晋升到 KAT Trace Library 的能力。它不隐式优先主线程；若产品需要“首个主线程帧”，必须以独立且表意准确的 Workflow 发布。它只接受必填字符串 `process_name`，由 Input Compiler 发布为 `--process-name`；调用方负责提供精确进程名，第一版不为此增加直接 Dataset query 或预运行进程发现 Interface。私有 helper 以 Perfetto 的 `thread_executing_span`、`wakeup_graph` 与按时间窗裁剪的 blocker 遍历为语义基线，Trace Streamer Adapter 只显式保留已验证的 OpenHarmony 差异；线程内部 ID 与时间窗不成为 Workflow 用户输入。唯一 Output `scheduling_dependencies` 按时间发布互不重叠且完整覆盖所选帧的 blocker 归属区间；其时间列是 `clock_domain`、`clock_value` 与 `duration_ns`，frame 侧列是 `frame_thread_id`、`frame_thread_name`、`frame_thread_state`、`frame_io_wait` 与 `frame_blocked_function`，blocker 侧列是 `blocker_thread_id`、`blocker_thread_name`、`blocker_process_id`、`blocker_process_name`、`blocker_thread_state`、`blocker_cpu`、`blocker_io_wait` 与 `blocker_blocked_function`。两个 `blocked_function` 只描述各自线程的实际状态，不沿 wakeup 链继承。内部 span、wakeup graph、节点/边 ID、depth、遍历状态与近似 callstack 不成为 Run Output；已有目标帧却无法形成完整归因时失败，不发布带缺口的 best-effort 结果。精确进程名不存在，或进程存在但没有已完成且持续时间为正的 actual frame，都是未取得分析目标的 Workflow failure，不生成零行 Output、合成行或 Run；因此成功 Output 必然非空。

该 Workflow 精确声明 `args`、`data_dict`、`frame_slice`、`instant`、`process`、`thread` 与 `thread_state` 七张 Required tables，并由四组有界 Reader 查询取得目标帧、线程元数据、线程状态和 wakeup 关系。`thread_state` 已拥有状态、区间与 CPU，`sched_slice` 的旧消费者 priority、二次 Running 证据和自创 classification 均已删除，因此它与近似 `callstack` 都不是依赖。

## Workflow Runtime–Workflow interface

Workflow Runtime 与单个 Workflow 之间的 Interface。Runtime 根据 Workflow 函数签名和装饰器把原始 Workflow arguments 解析为 effective Workflow input values，把唯一 Workflow Context 作为第一个位置参数、其余值作为关键字参数传入普通同步 function，并接收 `DataFrame | dict[str, DataFrame]`；裸 DataFrame 在返回边界立即规范化为 `{"main": dataframe}`，此后 Runtime 只处理具名映射。Workflow return annotation 可以按普通 Python 惯例自愿书写，但 KAT 不要求、不解析，也不把它当作 Output 清单；Input Compiler 不能因整体解析 annotations 而让 return forward reference 影响 inspection。Rust CLI 不参与 Workflow 参数或返回值语义。

## Intra-PACK Workflow composition

同一 PACK 内的 Workflow 可以通过私有 Python helper 和 DataFrame 组合复杂流程。Run Output 是随 Run 一同发布的持久输出，不是 Workflow 内部组合的中间接口；第一版禁止跨 PACK 依赖、import 和调用。

## Workflow arguments

`kat run` 中位于 `--` 之后、由 KAT CLI 不加解释地转交 Workflow Runtime 的命名参数序列。每个 Workflow 用户输入都按 Click 常规形式从精确 Python 参数名得到具名 long option；bool 额外使用 Click 的反向 flag pair。调用方直接使用 PACK inspection 中 Compiler 发布的 `option` 与 bool `negative_option`，不自行实现命名或冲突规则。Workflow Context 不生成选项，第一版 argv 没有位置参数、短选项、手工别名或 bool 文本值；这与 Python callable 内部允许 positional-or-keyword 用户参数不冲突。arguments 是调用时的原始文本表示，不承载类型、默认值或业务校验语义；这些语义只属于实际选中的 Workflow Interface。
_Avoid_: params file、Rust 解析的 Workflow flags

## Workflow input values

Workflow Runtime 根据函数签名和装饰器从 Workflow arguments 得到的具名、带类型 effective values，包括补齐的默认值和领域类型规范化结果。它们只表达单次 Workflow execution 的少量控制选择；数据属于 Dataset，稳定策略属于 PACK，复杂嵌套结构不属于 Workflow 输入。第一个 Workflow Context 参数不属于用户输入，私有 helper 调用也不受这一 Interface 限制。

## Run

一次成功 `kat run` 发布的生产执行结果，包含唯一 Run Manifest 和至少一个 Run Output；它是 Workflow 持久生产输出的唯一载体。Rust KAT CLI 在启动 Workflow Runtime 前预分配私有候选 UUID 和 KAT Data Home 下的 `runs/<candidate-id>/`，并拥有请求、Operation log、随机临时 Runtime Response 路径和进程生命周期；Python Runtime 只在 request 指定的候选目录中执行 Workflow、写出 Output 与一次性 Runtime Response，不自行分配生产路径或写最终 Run Manifest。只有 CLI 完成进程与 Runtime Response 验证、完整交付 Operation log，并把自己持有的 request facts 与 Runtime Response 的 success `result` 中新产生的事实合成为 Run Manifest 并持久发布唯一 `manifest.json` 后，候选 UUID 才成为公开 Run ID、候选目录才成为可由 `kat query` 寻址的 Run。任一步失败都不产生 Run；日志、候选目录、request、随机残留和未发布文件只是不参与查询的诊断证据。KAT 不维护额外 Run 状态、catalog 或 registry，删除已发布 Run 目录即删除 Run。

## Run ID

一次候选执行成功发布时才成立的 UUIDv7，也是 KAT Data Home 中已发布 Run 的目录名和 `kat query --run` 的唯一身份。CLI 在执行前预分配同一个 UUID 供候选目录、Runtime Request 与 Operation log 路径使用；最终 `manifest.json` 发布后，成功 `kat run` 的 KAT Response 才以 `run_id` 返回它。failure `kat run` 不含 `result`，也不把候选 UUID 称作或暴露为 Run ID。打开既有 Run 时必须验证 Run 目录名与 Run Manifest 中的 ID 一致。Run ID 不编码 PACK、Workflow、Dataset、时间路径或成功状态。

已发布 Run ID 还可直接定位 `<data-home>/logs/run-<run-id>.log`。发布前的 UUID 只是私有候选；失败操作只以自己的顶层 `log_path` 暴露诊断日志，不建立候选 ID Interface。

## PACK test execution

`kat_run` 每次调用都在 pytest `tmp_path` 下启动一次临时 PACK test execution。它复用 `kat run` 的候选目录和 Output 持久化布局，但不发布 Run、不进入 KAT Data Home，也不能被 `kat query` 按 Run ID 寻址；成功执行产生的 Output eager 读回为 `dict[str, pyarrow.Table]`，该临时执行现场的生命周期由 pytest retention policy 管理。

## Run Manifest

已发布 Run 中精确的 `manifest.json`，是该 Run 唯一的持久清单。Runtime 先把处于 success 分支的 Runtime Response 写入 CLI 分配的随机临时路径；CLI 验证候选目录名与候选 UUID、Runtime 以 `0` 退出、Runtime Response 处于合法 success 分支，以及非空 `outputs` object、合法 Output name key 和唯一 `output_id`，并确认 Operation log 完成交付后，通过封闭 typed constructor 把 CLI-owned 候选 UUID、PACK、Workflow 与可选 canonical Dataset path，同 Runtime-owned effective inputs 和 Outputs 合成为唯一的内存 Run Manifest；constructor 的两组 typed 参数在类型定义上拥有互斥字段；Runtime 未知字段在此前的严格解码中失败，不做值层字段碰撞检查。CLI 把该对象写入自己创建的同目录临时文件，再持久化为最终 `manifest.json`；只有此时最终文件才成为已发布 Run Manifest。最终文件包含 Run ID、PACK name、Workflow name、可选的 canonical Dataset path、规范化后的 effective Workflow input values，以及以 Output name 为 key 的非空 `outputs` object；每个 value 精确只有 `output_id`、有序 `columns` 和 `row_count`，不重复 `name`，也不保存 Output 顺序。Run Manifest 不包含 `success`、`failure` 或其他执行状态，也不持久化自动 Preview 或完整 Arrow Schema；`columns` 精确复用 Query Result 的 `{name, type}` object array，完整 Arrow Schema 只由对应 Parquet 权威保存，不在 JSON 控制面复制；Dataset path 缺席本身完整表示本次 Run 未提供 Dataset，不另存 `has_dataset` 或空值。CLI 把 Output ID 当作 Runtime 的不透明引用，不从中推导或预检物理文件；Runtime 只在后续 Output Query 解析该引用并打开对应文件时报告文件缺失、损坏或不可读。Output ID 只存在于 Runtime Response、最终 `manifest.json` 与 Runtime Request。CLI 在 persist 前从同一个内存 Run Manifest 纯投影并验证公开 `result` candidate，不重新读取磁盘，也不再从 Runtime Outputs 建立平行投影；只有该对象成功持久发布为最终 `manifest.json` 后，才把这个 candidate 写入 success Response。Public Outputs 仍按同一 Output name 新建 map，每个 value 固定只有 `columns` 与 `row_count`；`output_id`、effective inputs 与其他 Manifest 私有事实在公开类型中没有位置。`run_id` 同样取自这份已发布 Manifest。Run 至少包含一个 Table Output，零行输出仍显示其名称、完整 `columns` 和 `row_count = 0`。Runtime Response 的 failure 分支只描述失败与诊断，不能产生 Run Manifest；request 和随机临时文件不作为恢复来源，KAT 不再维护平行的 `run.json`、Run catalog 或 registry。Run Manifest 描述执行及输出，不声称已经给出 Analysis Result。

_Avoid_: Run summary、最终 summary

## Run Output

一次 Run 持久保存的命名输出。多输出在逻辑上全有或全无：Runtime 先完整校验返回映射，再执行并写出各 DataFrame，只有全部成功才把 Output ID 写入临时 Runtime Response 的 success `result`；CLI 只有在整个操作成功后才通过最终 `manifest.json` 一次性发布它们。任一项执行、写出或外层交付失败都会使 `kat run` 操作失败且不发布 Run，已经写出的文件与临时 Runtime Response 也不成为 Run Output 或 Output Query 的输入。第一版 Run Output 只属于本次 Run，不写入 Dataset，也不承诺跨 Run 复用；删除 Run 即删除其 Outputs。它是可继续查询或解释的程序产物，不等同于用户可直接阅读的分析结论。
_Avoid_: Artifact、Result

## Output ID

Workflow Runtime 为候选执行中的一个 Table Output 产生的私有不透明引用；最终 `manifest.json` 通过 Output name 发布后，它才成为属于 Run 的 Output 引用。后续 `query_run` 把 canonical Run path 与该引用原样交回同一个 Runtime，由 Runtime 私有地解析引用、定位并打开物理文件；CLI 不解释其格式或物理布局，用户和 PACK 也不以它寻址 Output。

## Output name

Workflow 返回的具名 DataFrame 使用的逻辑机器名称，同时作为 Python 字典 key；Run 发布后，同一名称也成为 Run Manifest 字段和 `output.<name>` 中的 SQL table identifier。它必须完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，例如 `main`、`thread_cpu_time_by_cpu` 或 `top_10_blockers`；KAT 不自动转小写、清理或修复，也不接受需要 SQL quoting 的 Unicode、空格、连字符、点、前后或连续下划线。限定名称由 DataFusion 解析，KAT 不维护 SQL 关键字黑名单。第一版不设名称长度上限，也不因 Windows 设备名限制逻辑名称；Runtime 使用不含 Output name 的 Output ID 私有定位跨平台物理文件。

## Output Query

模型或用户通过 KAT Skill 以 `kat query --run <run-id> --sql <sql>` 对一次成功生产 Run 发起的有界后续查询。`--run` 与 `--sql` 都必填，SQL 恰好包含一条 DataFusion statement；第一版不接受位置 SQL、stdin、`--sql-file`、多 statement 或查询参数绑定。Rust KAT CLI 只查找并严格验证该 Run 精确的 `manifest.json`，不读取原 request，也不扫描随机临时文件或其他残留。该文件缺失时直接报告 `Run <run-id> 不存在`；候选目录、随机临时文件、Operation log 和其他残留都不建立 Run，也不用于推断执行中、失败或删除等历史状态。文件存在但验证失败时则明确报告 `Run <run-id> 已损坏`，不把损坏伪装成不存在。CLI 把 canonical Run path 与全部已发布 Output name 到 Output ID 的映射原样写入 `query_run`，不解释 Output ID 或预检物理文件。Run Manifest 没有 Dataset reference 时，CLI 直接写入 `not_provided` dataset；有 reference 时才由 Dataset Storage 尝试重新解析该 canonical path，并写入 `available` 或 `unavailable` dataset。Workflow Runtime 不读取 `manifest.json`、不扫描 Run 或 Dataset 目录，只用自己的私有布局逐项解析 Output ID 并注册 `output.*`，只在 Dataset 可用时注册 `dataset.*`，再把未解释 SQL 交给 DataFusion。没有提供或当前不可用的 Dataset 都不会阻断只访问 `output.*` 的查询；后者的原路径与可读 cause 保留在 request 中，但只在下述类型化 Dataset failure 直接成立时用于诊断。已发布 Output 文件缺失、损坏或不可读时，当前 query 在 Runtime 中整体失败。同一路径后续被整体覆盖或删除后重建时，旧 Run 的 `dataset.*` 读取其当前数据，而不改变 `output.*`；路径缺失或不再是有效 Dataset 时不注册 `dataset.*`。Output Query 可以调用 `kat_convert_clock`，其 Resolver 只读取这份查询当下可用的当前 Dataset，不判断参数来源，也不保存或恢复 Run 当时的时钟证据；Dataset 已覆盖时由用户承担对历史 Output 使用当前证据的语义后果，没有提供或当前不可用时只有实际调用换算才失败。成功 query 的 `result.dataset` 始终以 `not_provided`、`available` 或 `unavailable` 明确报告 Run 的 Dataset reference 及其当前状态；failure query 不含 `result`；只有 Runtime 对 `dataset.*` 或依赖 Dataset 证据的能力执行类型化解析、并由 `not_provided` 或 `unavailable` 状态直接选择失败分支时，Diagnostic 才说明该状态；canonical Run path、Output ID 和物理布局不对外呈现。该类型化 Dataset failure 说明状态本身不会禁用健康的 `output.*`，并给出只查询 `output.*` 或重新运行 Workflow 的帮助；仅 `unavailable` 状态可引用原路径与 Dataset Storage cause 并提示恢复 Dataset。普通 SQL 语法、Output、函数或其他 DataFusion failure 即使碰巧伴随 `not_provided` 或 `unavailable` request，也只保留自己的可靠 cause，不附加 Dataset 上下文。KAT 不解析 SQL 或错误字符串来猜测引用关系。查询只允许读取，并受行数、最终 compact success KAT Response 的 UTF-8 字节数和执行时间限制，记入与该 Run 关联的本次 `query` Operation log。Skill 默认先用投影、过滤、聚合和排序缩小事实，再显式 `LIMIT` 少量明细；超过任一限制时整个查询失败并提示缩小查询，不返回部分结果。KAT 不按 Arrow buffer、Parquet 文件或特定模型 token 计算上下文边界，不自动改写 SQL、注入 `LIMIT`、静默截断、分页或返回替代 artifact。

## Query Result

一次 Output Query 完整落在既定行数、最终 compact success KAT Response UTF-8 字节数和执行时间边界内时返回的临时结构化数据。成功 `kat query` 的 operation-specific `result` 始终且只包含 `dataset`、`columns` 与 `rows`。`dataset` 始终存在并精确使用三种形状：Run 未提供 Dataset 时为 `{"status":"not_provided"}`；记录的 Dataset 当前可用时为 `{"status":"available","path":"..."}`；记录的 Dataset 当前不可用但纯 `output.*` 查询仍成功时为 `{"status":"unavailable","path":"...","cause":"..."}`。后两者描述查询当下的 Dataset，而不是 Run 输入快照。这个固定字段不因 SQL 只访问 `output.*` 而省略，CLI 不解析 SQL，也不增加重复的 `current` 字段。`columns` 是按 SQL 结果顺序排列的 `{name, type}` object array，`type` 使用 Arrow 的可读类型字符串，不另建 KAT 类型语言；`rows` 是 positional JSON array 的 array，每行长度必须与 `columns` 完全一致，值按同一位置对应。Arrow `Utf8`、`LargeUtf8` 与 `Utf8View` 的每个非 null 值都按内容原样投影为普通 JSON string，只使用标准 JSON escaping；`columns[].type` 保留实际 Arrow 字符串类型，KAT 不截断、文本规范化或增加 tagged wrapper。Arrow `Int64` 与 `UInt64` 的每个非 null 值统一投影为无前导 `+` 的十进制 JSON string，负数保留 `-`；不根据值是否落在 JSON interoperable safe-integer 范围内切换为 number。对应 `columns[].type` 仍为 `int64` 或 `uint64`，所以字符串只是无损传输形式；`Int8/16/32` 与 `UInt8/16/32` 仍使用 JSON number。`Timestamp(ns, UTC)` 的非 null 值投影为规范 UTC RFC 3339 JSON string：使用 `Z`，最多九位小数秒并删除尾零，整秒不输出小数部分；column type 仍保留 Arrow timestamp 类型。其他单位、无时区或非 UTC 的 Arrow Timestamp 使整个 query 失败，并提示调用方先严格 cast 为 `Timestamp(ns, UTC)` 或显式转成 Utf8；KAT 不猜时区、不输出 epoch 整数、不使用 `try_cast`、tagged object 或格式选项。`Decimal128` 与 `Decimal256` 先用 arrow-rs 校验每个非 null 值符合 column precision 和 scale，再复用其 formatter 生成定点十进制 JSON string；任何非法值使整个 query 失败，不转换为浮点数或 JSON number。`columns[].type` 保留 precision 和 scale，KAT 不重写 Arrow 的 decimal 校验或格式化算法。有限的 Arrow 浮点值使用 JSON number；任何结果单元格为 `NaN`、`+Infinity` 或 `-Infinity` 时，整个 query 在写 stdout 前失败，不把它静默改成 `null` 或 string，也不返回其余部分。null 对所有 nullable column 仍是 JSON `null`。这些规则只属于 Skill-facing Query Result 标量投影，不改变私有 Runtime IPC 的已知端点类型。该结构保留列顺序与重复列名，列名和类型只出现一次；实际 stdout 使用 compact JSON，文档才 pretty-print。Query Result 不重复 `row_count`，不含 `truncated`、分页 token 或 artifact reference。超长字符串仍只由完整候选结果的既有字节上限处理；KAT 不局部裁剪。Binary、Date/Time/Interval、List、Struct、Map 等尚无真实输出需求的类型，以及不支持的 Timestamp、非法 Decimal、非有限浮点或其他投影错误，都使当前 query 整体失败并提示 PACK/SQL 先显式投影为已支持标量，不返回部分 Query Result。`query_run` Runtime Response 的 success `result` 精确只有 `columns` 与 `rows`，不含 `dataset`；CLI 严格解码该私有类型，再把自己持有的当前 Dataset 状态加入新构造的公开 `result`。Runtime 回显 `dataset` 属于未知字段并使 IPC 失败。CLI 以候选成功 KAT Response 的最终 compact UTF-8 大小检查字节上限，通过后才写 stdout，否则改为 failure KAT Response。它不创建新 Run、Run ID、Table Output 或 Output ID，也不写回 Dataset 或原 Run。需要继续追问时由模型发起另一条独立 Output Query；若所需数据无法通过小型核验查询取得，应新增或修改 Workflow 产出合适的 Table Output。

## Table Output

由 Workflow 返回的 DataFusion DataFrame 写成的结构化 Run Output。第一版只支持 Table Output：Workflow 可以直接返回一个 DataFrame，Runtime 将其规范化为 Output name 固定为 `main` 的单元素映射；需要领域名称或多个输出时返回非空 `dict[str, DataFrame]`，其中 key 必须是合法 Output name。要发布 Run，规范化后必须至少包含一个 Table Output；`None` 与空字典通常表示漏写 `return`，在返回边界直接失败。没有匹配记录使用具有确定 Schema 的零行 DataFrame 表达，仍然是有效 Table Output。名称不从函数、文件路径或 Workflow identity 推导，也不在 decorator 中覆盖。KAT 不预先声明或逐项注释 Output，不校验静态 Output name、列名或 Schema；名称、列名和执行时实际 Schema 承担机器可读语义，额外背景只写在普通 Workflow docstring 或源码注释中，KAT 不解析。返回边界先完整校验真实容器、全部 Output name 与 DataFrame value，并拒绝 PyArrow Table、list、tuple、generator、标量和其他容器；所有 DataFrame 执行和写出都成功后，Runtime 才把完整映射写入临时 Runtime Response 的 success `result`；CLI 通过最终 `manifest.json` 完成整体发布。规范化完成后，持久化、Run Manifest 与 Output Query 始终只消费这份完整具名 DataFrame 映射。

## Analysis Result

面向用户的可读判断、报告或结论。Workflow Runtime 生成 Run Outputs 与 Runtime Response，KAT CLI 发布最终 Run Manifest；KAT analysis flow 把这些输出交给模型并组织有界追问，由模型形成 Analysis Result。

## Workflow SQL

第一版 PACK Workflow 直接手写 DataFusion SQL，并通过 `ctx.sql(sql, **params)` 做参数绑定，从 Table Grant 以裸表名注册的不可变 Source tables 派生新的 DataFrame。它接受一条由 DataFusion 规划后确认不包含 DDL、DML、COPY 或 session mutation 的 SQL；只读的 `SHOW`、`DESCRIBE` 和 `EXPLAIN` 可以使用。KAT 通过 DataFusion 的 SQL options 配置并调用这一边界，不自建 SQL parser、语法白名单或语句分类。Source 在这里是来源与生命周期角色，不是 SQL namespace；Output Query 始终把 Run Outputs 注册为 `output.*`，只有 `available` Dataset 才另外注册 `dataset.*`。该 Interface 没有持久或临时写入、模块级 `kat.sql` 或 SQL builder。

## Workflow SQL parameter

`ctx.sql` 中以 `$name` 出现在值表达式位置、由同名关键字参数通过 DataFusion `param_values` 绑定的标量值。第一版沿用 DataFusion 的命名参数行为，不另行解析占位符：缺少参数使查询失败，多余但类型合法的参数由 DataFusion 忽略；KAT 负责把失败呈现为可操作的诊断，不维护参数名称集合。公开参数类型只包括 `bool`、有符号 64 位范围内的 `int`、有限 `float`、`str`、`kat.Duration` 和 `kat.WallClockTimestamp`；它们依次转换为 Boolean、Int64、Float64、Utf8、纳秒 Int64 和 Arrow `Timestamp(ns, UTC)`。`None`、`bytes`、`Decimal`、集合、Python `datetime` 或 `timedelta` 以及任意 `pyarrow.Scalar` 都不属于该 Interface；SQL 空值直接写作 `NULL`，可选条件由 Workflow 选择 SQL 分支。参数不执行字符串替换，也不解释为表名、列名、SQL 片段或需要注册临时 View 的 DataFrame。
