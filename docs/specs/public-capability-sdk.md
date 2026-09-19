# 官方公共能力 SDK：发现、知识与独立交付

状态：实现已落地，Windows 本地验证通过；Linux 与完整发布载荷验收待 CI，详见文末证据。
代码依据：`8d0a74e2c11569e4e36daeaa23d4a92e76b79cce`。
关联：[Issue #296](https://github.com/maokelong/kat-cli/issues/296)。

## 目标与非目标

将各领域需要复用的具体 Provider、Workflow 和公共 Python 函数收敛为官方 SDK。用户在 KAT 使用的 Python 环境中通过 pip 安装或升级 wheel；Skill 运行时通过现有 `kat` 命令发现公共 Workflow、Provider 及对应知识，通过 KAT 直接执行或组合调用 Workflow。公共函数由 Python 直接导入，AI 阅读随包 Markdown 了解用法。

SDK 不包含 CLI、Runtime、Context、装饰器、执行协议或另一套表类型。框架提供的 `kat` 作者 API 和 Data Provider Toolkit 继续由框架维护；SDK 使用这些接口。首版只支持官方能力，不建立第三方插件体系、自动在线更新、独立执行宿主或新的公共函数发现命令。源码仓库拆分、PyPI 发布、原生解析器合并、额外 Python/平台支持均不属于本切片。

## 首个交付切片

1. 抽取现有 `FtraceProvider`、`TraceStreamerProvider` 的实现、声明、知识和相关行为测试。
2. 引入 `kat_sdk` 导入命名空间，迁移仓库内消费者、示例、测试和文档；不保留旧 Provider 导入别名，不保留两份实现。
3. 将 SDK 根目录作为一个正式公共 PACK 接入统一发现与执行。使用仅供测试的 SDK/PACK fixture 验证真实 Workflow 发现、直接执行和跨 PACK 调用。
4. 交付随包知识、API Markdown 生成、wheel 构建与双平台安装/升级验证。

生产公共 PACK 可以暂时没有 Workflow；现有合同允许零 Workflow。不会为了填充目录把 `summarize-ftrace`、`summarize-native-hook` 等示例自动晋升为受支持公共能力。公共函数也只在出现明确复用需求时纳入，不创建占位算法或空目录。

## 内容与目录

源码构建单元采用如下逻辑布局；具体源码位置在实现时与现有构建布局对齐，不在本规格中授权整仓移动：

```text
sdk/
├─ pyproject.toml
├─ pack.toml
├─ providers/                 # 具体公共 Provider
├─ workflows/                 # 一个公共 PACK 的入口，内部按领域组织
├─ libraries/                 # 普通公共 Python 模块
└─ knowledge/
   ├─ index.md
   ├─ providers/
   ├─ workflows/
   └─ libraries/
```

不增加 `packs/` 层。构建使用标准 Python backend 将源码映射为可导入的 `kat_sdk`；安装后的 `kat_sdk` 资源根直接具有 `pack.toml`、`workflows/` 和 `knowledge/` 的相对关系。构建配置本身不必进入 wheel。普通包初始化文件按 Python 打包需要设置，SDK 根导入保持轻量。

`pack.toml` 沿用现有 `name/title/description/owner` 合同，不添加 SDK 版本、Workflow 列表或依赖字段。PACK 身份由 manifest name 给出，SDK distribution 版本来自标准包元数据；文中命令使用 `<sdk-pack>` 指代实际名称。

`workflows/` 沿用现有声明式入口规则：每个 Python 入口定义自身的一个 Workflow，目录层次只组织源码，不改变显式 Workflow name；不增加 `__init__.py`，不直接 import 其他 Workflow 入口作为复用方式。公共 Provider 和函数从 `kat_sdk.providers`、`kat_sdk.libraries` 导入，跨 Workflow 组合使用 `ctx.run()`。

## 框架边界与依赖

- `Context`、`RunError`、装饰器、时间类型和 `kat.dataprovider` Toolkit 暂留框架。`Table` 是 Runtime 精确类型合同，`Catalog` 是组合调用返回合同，不在 SDK 内复制或改名。
- FtraceProvider 改用已有公开 Toolkit 接口，消除对框架 `_fusion/_parquet/_table` 的私有导入。底层 `kat_datasource.text_ftrace` 仍是外部原生依赖，不打入 SDK。
- TraceStreamerProvider 保留现有行为和显式外部 executable/SQLite 输入。它当前对 `_write._rename_no_replace` 的私有依赖必须在迁移时解除，采用最小必要的职责调整，不搬走整个框架 writer。
- wheel 声明实际 Python distribution 依赖，KAT 发布装配提供经过验证的依赖闭包；外部可执行工具的要求进入 Provider 文档。不能仅凭 `import kat_sdk` 成功宣称 Provider 可用。
- 首次接入需要支持该方案的 KAT 基线。SDK 独立版本只在明确验证的框架及依赖范围内升级；最低版本和依赖范围由实现及真实测试确定，不虚构版本号，不承诺兼容任意旧 CLI。
- 首版使用当前 KAT 的 CPython 3.14、Windows x86_64 与 Linux x86_64 环境。SDK 的 Python 内容可构建通用 wheel，底层依赖仍分别验证，不因 wheel 是纯 Python 就免除平台验证。

## Workflow 发现与执行

框架通过当前 KAT Bundled Python 定位已安装 SDK 的真实资源目录，将其作为精确 PACK candidate 加入现有发现集合。SDK 正常 pip 解包安装，不复制到 Data Home，也不让用户为每个领域重复安装一份。首版不增加 ZIP 内 PACK 执行支持。

保持现有 static manifest 解析、canonical directory 去重和同名冲突规则：相同目录重复加入去重，不同目录同名失败，SDK 不拥有覆盖优先级。目录定位不导入具体业务模块、不运行分析。

```text
kat inspect
kat inspect workflow --pack <sdk-pack>
kat inspect workflow --pack <sdk-pack> --workflow <workflow>
kat session create
kat run --session <id> --pack <sdk-pack> --workflow <workflow> -- <业务参数>
```

跨 PACK 调用保持 `ctx.run("<sdk-pack>", "<workflow>", **inputs)`，返回现有只读 Catalog。inspection、顶层执行、嵌套执行和 PACK 测试必须共享 SDK 所在的发现范围，不能只给列表增加入口。

SDK 未安装是正常状态：跳过 SDK 候选，保留 Skill、Data Home 与 `--pack-dir` 的所有既有 PACK；公共 Provider 列表为空，指定不存在的公共 Provider 返回未找到。卸载 SDK 后框架自有操作和不依赖 SDK 的 PACK 继续可用。没有 Python Host 时仍可执行原有纯 manifest PACK 发现，运行 Workflow 继续遵循框架原本的 Host 要求。已安装 SDK 的资源损坏仍报告安装/资源问题，不将损坏误判为未安装。

## Provider 发现与知识

继续使用 `kat inspect provider` 和 `kat inspect provider --provider <name>`。公共范围不依赖任何领域 PACK，也不因为某个无关 PACK 损坏而不可用。`--pack` 继续选择 PACK 自有范围，不自动合并或回退。

SDK 拥有公共 Provider 模块清单；框架从固定 SDK 入口取得清单，不硬编码 Ftrace/TraceStreamer 等具体模块。模块清单只负责定位，不重复维护装饰器已有的名称、描述和 Guide；增加 Provider 只更新 SDK 内容。无需扫描所有已安装 distribution 或引入第三方注册框架。

列表仍返回名称和描述；详情仍返回声明、真实 `kat_sdk` 导入位置和原始 Guide Markdown。Provider Guide 由声明引用 `knowledge/providers/` 资源；inspection 只读取声明和知识，不实例化 Provider、连接来源或执行查询。

Workflow 与 Provider 的实现、声明和知识一起版本化。新增能力的发现、文档读取和实际调用必须相互一致。每次命令读取当前安装版本，不将知识复制进 Skill 或另建持久索引。

## Markdown 与 AI 阅读入口

| 内容 | 权威来源与交付方式 |
| --- | --- |
| 参数和返回类型 | Python 类型注解 |
| 语义、单位、限制、异常、示例 | Python docstring |
| API 参考 Markdown | 构建时从显式公开接口生成 |
| 导航、教程、Provider 来源知识、Workflow 分析 Guide | 手写 Markdown |

按类别在 `knowledge/providers`、`knowledge/workflows`、`knowledge/libraries` 组织内容。API 参考与 Guide 用不同文件维护，生成器不得覆盖手写内容；Guide 仍承担来源语义或输出解释职责，不能由函数签名替代。

`knowledge/index.md` 描述能力用途并通过相对链接指向详细文档。公共函数不注册为 KAT 能力；AI 根据 Skill 公共说明中的稳定定位方法，通过 KAT 当前 Python 找到该首页，再阅读模块/函数用法。此入口服务运行过程，不引入 kat-author 开发工作流。

使用成熟工具解析类型和 docstring，不自行实现通用 Python 解析器。实现前验证工具对当前语法、中文和真实 Markdown 输出的支持并固定构建依赖；工具选型属于实施技术验证，不意味着已选择或验证某个生成器。文档生成依赖不进入 SDK 的运行时依赖。

## 构建与发布

拟议脚本职责：

- `build/build_sdk_wheel.py`：在临时构建目录生成 API MD，校验声明/知识引用和链接，使用标准 backend 构建 wheel，验证安装布局、元数据及资源完整性，输出 wheel 与 SHA256。
- `build/verify_sdk_install.py`：在隔离的 KAT 测试部署中安装指定产物，执行发现、知识读取、实际使用和升级验证。该环境拥有真实框架与必要原生依赖，不用裸系统 Python 冒充受支持宿主。

复用现有工具锁定、标准构建和归档校验方式；SDK 内容校验与私有 Workflow Host wheel 校验分离，不复用其中要求携带 Runtime 的假设。脚本必须支持当前双平台运行条件，不能照搬现有 Linux 下载默认值。

首版发布 GitHub Release wheel 和校验摘要，不发布 PyPI。用户通过当前 KAT Python 执行 `-m pip install --upgrade <wheel路径或URL>`；不是对未发布的索引包执行升级。SDK 独立版本不加入现有所有组件必须同版本的校验；KAT 成套发布记录并安装已验证的 SDK 版本及依赖。正常分析不主动安装或更新。

生成的 API MD 是构建产物，进入 wheel；源码维护声明、docstring、手写知识和必要生成配置，不提交临时构建目录。实际发布资产应与验证过的 wheel 一致。

## 验收与证据

| 场景 | 必须提供的结果 |
| --- | --- |
| 未安装及卸载 SDK | 原有三个来源 PACK 的发现、自有 Provider、直接/嵌套执行、查询和测试正常；纯 manifest 发现不新增 Python 要求 |
| 干净 KAT 部署安装真实 wheel | 资源布局正确，依赖检查通过，两个 Provider 可导入并实际查询 |
| 公共 Provider list/detail | 正确名称、摘要、kat_sdk 导入路径及非空知识；不构造来源，不依赖领域 PACK |
| 正式 SDK 无公共 Workflow | 公共 PACK 可发现，Workflow 完整列表为空，不虚构业务能力 |
| 测试 SDK 含 Workflow fixture | 列表、参数、Guide、直接执行、跨 PACK 调用及 kat test 路径一致 |
| 同名/损坏资源 | 沿用明确失败语义，不覆盖、不回退、不返回部分列表 |
| 公共函数文档 fixture | 从首页相对链接找到真实 import、签名、语义和可执行示例；无函数发现命令 |
| v1 → v2 SDK 升级 | 新增测试 Workflow/Provider 与更新 MD 可见可用；CLI/Runtime 版本不变；旧 SDK 文件按 pip 正常规则替换 |
| 升级中的既有能力 | 先前支持的调用继续有效，新增知识与执行目标同版本 |
| 迁移回归 | 仓库消费者使用新路径，旧 Provider 路径移除，两个 Provider 行为与原生解析结果不因迁移改变 |
| Windows/Linux | 在 CPython 3.14 的当前支持环境分别完成安装和真实调用，不能只检查 wheel 清单 |

只有测试 fixture 用于验证未来扩展和公共函数文档链路，不把未批准的示例能力放入发布 SDK。外部解析器缺失时不得冒称相关真实执行验证通过；PR 列出实际命令、环境、SDK/框架版本、wheel 摘要及结果。

## 既有决定与实施前更新

本规格采用用户本轮明确确认的范围，不恢复旧 SDK 方案：

- Issue #280 仍描述独立仓库与框架 API 迁移，范围不同，不作为本次迁移授权；其历史记录保持不变。
- Issue #284 已废弃；不恢复其 Runtime/原生 Datasource 合并、统一命名空间整体迁移方案。
- ADR-0045 和 CONTEXT 中公共能力只能随私有 Host wheel 原子发布的部分，需要在实现时局部替代；框架 API 与 Runtime 的现有归属继续保留。
- ADR-0049 中 `kat.trace` 原子发布和不建立 common PACK 的表述，需要与本次公共 PACK/公共库所有权重新对齐；真实消费者验证后才晋升公共能力的原则保留。
- ADR-0081 中公共 Provider 位于平台私有 wheel 的归属更新为官方 SDK；公共发现与 PACK 自有范围隔离、Guide 随所有者版本化的原则保留。
- ADR-0007 的固定发现位置需纳入 SDK 精确根，ADR-0017 的入口校验继续保留。ADR-0084 的当前 Host pip 安装能力沿用，不在本切片增加整套 KAT 替换后的环境迁移。

用户随后已授权实现。源码位于 `kat/sdk`，所有权变化由 ADR-0085 记录；SDK 版本为 0.1.0，首次支持该方案的框架基线为 0.1.1rc13。实际发布仍需明确的双平台验证证据，本次不自动发布 Release。

## 本地验证记录（2026-09-19）

实现已落地；下列结果仅代表已实际运行的 Windows 验证，Linux 与完整发布载荷验收仍待 CI。

环境：Windows x86_64、CPython 3.14.0。SDK 0.1.0；框架与 Datasource 为 0.1.1rc13。本机使用已安装的 Rust/Cargo 1.97.1；仓库锁定的 1.95.0 由 CI 验证，本机结果不冒充锁定工具链结果。

| 验证 | 实际结果 |
| --- | --- |
| `cargo test -p kat-cli --locked --tests`（本机 stable 工具链） | 167 passed，11 ignored |
| 另行运行五条 real installed Host 测试 | 5 passed：知识读取、组合调用、业务错误传播、scratch 生命周期、PACK 测试 |
| `python -I -B -m pytest kat/platform/workflow/tests kat/sdk/tests -q` | 209 passed，294 subtests passed |
| 可选 SDK 修正后重跑 `test_runtime_process.py` | 30 passed，37 subtests passed |
| `python -m unittest discover -s build/tests` | 88 passed，含静态中文 API 生成、无初始化文件的 Workflow、Guide/链接缺损和手写文档保护 |
| `cargo clippy -p kat-cli --locked --all-targets -- -D warnings` | passed |
| `cargo fmt --all -- --check`、`git diff --check` | passed |
| `python -I -B build/verify_release_versions.py` | 0.1.1-rc.13 一致，SDK 版本独立 |
| `build/verify_sdk_install.py`，三个真实 wheel 与当前 CLI | passed；隔离宿主中完成安装、缺损资源/重名失败、Provider 查询、直接/嵌套/测试调用、函数文档以及 SDK v1 → v2 升级 |

最终候选为 `target/issue-296/sdk-candidate/kat_sdk-0.1.0-py3-none-any.whl`，SHA256：

```text
515b176056cf32ef2e753674782acc86786d5946c869d920fb192f37e6c91129
```

安装验证报告为 `target/issue-296/verification-optional/report.json`；对应日志为 `target/issue-296/optional-sdk-verification.log`。升级 fixture 为 `0.1.0+verify1` 与 `0.1.0+verify2`，只供测试；正式 SDK 未增加虚构 Workflow 或公共算法。升级后再次运行两个 Provider 的行为测试与 pip check，并验证框架版本和 CLI 摘要保持不变。

Trace Streamer 验证覆盖实际 SQLite 查询与受控解析器行为，不代表已用真实外部 Trace Streamer 完成完整 Trace 解码。独立 `SDK CI` 已配置为两平台下载同一候选并校验 SHA256 后验收；`Full CI` 和成套 Payload 构建也已接入 SDK。尚未触发远程 CI，未发布 wheel 或 Release；草稿 PR 为 #297。

可选 SDK 回归：真实隔离环境先不安装 SDK，验证 Skill、Data Home 和显式目录中的 PACK，再安装、升级并卸载 SDK，重复完整基线验证。两次均通过，且 CLI 摘要与框架版本保持不变。Payload 构建可省略全部 SDK 参数；只提供部分 SDK 参数会被拒绝。
