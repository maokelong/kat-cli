# Workflow 开发时的公共 API 文档

状态：已接受，正在按 [Issue #300](https://github.com/maokelong/kat-cli/issues/300) 实施；架构决定见 [ADR-0086](../adr/0086-api-documents-are-reviewed-author-references.md)。API 文档生成、审核、交付与使用规则已经收敛，本文同时记录针对已合入 PR #297 的修改和验证范围。文档修订治理、自动同步和发布校验等维护问题留待后续设计。

## 已确认的需求

- 框架侧 API 与 SDK 当前由两个独立 wheel 交付：`kat-workflow` 提供框架作者 API，`kat-sdk` 提供官方 SDK API。`kat-datasource` 是 SDK 可使用的底层依赖，不是本文所称的 SDK wheel。
- `base-api.md` 与框架 API wheel 版本绑定；SDK 的 `api.md` 与 `reference/` 与 SDK wheel 版本绑定。
- 两个 wheel 都不包含 API 文档；wheel 构建不负责把已审核文档装入 wheel。
- 同一 wheel 版本允许在人工审核后独立修正文案、示例和链接；新增、删除或改变 API 行为必须随新的 wheel 版本发布。
- 文档目录随 SDK 开发维护和完善。
- 文档内容尽可能少，重点说明有哪些能力。
- AI 开发新的 Workflow 前，必须先检查当前提供的能力目录，核对是否有可复用的公共能力；命中候选后再按需读取详细用法。
- 同时覆盖框架侧 Pack Authoring API 和 SDK API：`base-api.md` 单独存放，SDK API 文档根目录只包含 `api.md` 和 `reference/`。
- 简短文档仅承担能力发现，每项保留公开入口、一句话用途和详细用法链接；AI 命中候选能力后再按需读取详细说明。
- 调用签名、参数、返回值、错误、边界和示例放在详细说明中，不在能力目录重复展开。
- SDK 文档由 `kat-dev-sdk` skill 在 SDK 开发过程中生成到 SDK 源码目录，经人工审核后由用户自行复制到 `kat-author/references/`。框架侧 `base-api.md` 由框架自己的开发流程维护，`kat-dev-sdk` 只读取它，不生成或修改它。

## 文档结构与查阅入口

生成环境中的 SDK API 文档在 SDK 源码目录下再封装一层 `docs/`：

```text
kat/sdk/
  docs/
    api.md
    reference/
```

- `base-api.md`：框架 wheel 的基础作者 API 说明，由框架侧维护，不放在 SDK 文档目录中。
- `api.md`：SDK API 文档的统一入口和简短能力目录；每项包含公开入口、一句话用途和详细用法链接。
- `reference/`：每个公共模块一个详细文件，承载该模块全部公开符号的签名、参数、返回值、错误、边界及必要示例。

SDK 文档完成生成和人工审核后，用户自行将 `kat/sdk/docs/api.md` 与整个 `kat/sdk/docs/reference/` 复制到目标 `kat-author/references/`。`kat-dev-sdk` 不自动写入 `kat-author`，SDK wheel 构建不执行复制，Skill 装配也不从 SDK 目录隐式同步文档。

复制后的作者侧布局为：

```text
kat-author/references/
  base-api.md
  api.md
  reference/
    <module>.md
```

`kat-author` 只从自身 `references/` 读取这些文件，不回到 SDK 源码目录查找。

`base-api.md` 收录 `kat.workflow`、`kat.Context`、`Table`、`Schema`、`open`、`write`、`DataFusionProvider` 等框架作者能力。`api.md` 与 `reference/` 收录 `kat_sdk` 提供的 Provider、公共函数等具体 SDK API。两类文档可以互相引用，但不合并其 wheel 身份或版本。

AI 开发 Workflow 前先读取当前提供的 `base-api.md`，再检查 SDK `api.md`；命中候选能力后沿链接读取 `reference/` 中的详细说明。使用方默认信任这些文档已经与对应 wheel 匹配，不自行核对版本。

SDK 文档按公共模块生成。以下仅示意文件形态：

```text
api.md
reference/
  <module>.md
```

`api.md` 用一条记录说明每个 `kat_sdk` 公共模块能做什么，并链接相应 reference 文件。每个 reference 文件集中列出该模块 `__all__` 中的全部函数、类、异常和常量；不为单个符号再拆文件。

`api.md` 中的模块顺序严格沿用 SDK 源码中明确登记的公共模块顺序；每个 reference 中的符号顺序严格沿用对应模块 `__all__` 的声明顺序。生成器不重新按字母排序，使 SDK 作者可以在源码中表达推荐阅读顺序。

`api.md` 使用固定模板：标题、SDK wheel 版本，以及按公共模块排列的能力列表。每项只包含模块公开导入路径、一句话用途和对应 reference 相对链接。

每个 `reference/<module>.md` 使用固定模板：模块公开导入路径、一句话能力说明、完整公开 API，以及可选的经验证示例。函数和方法记录签名、参数、返回值、公开错误及必要的行为边界或副作用；公开类、异常和常量同样按模块 `__all__` 完整列出。文档不加入架构背景、内部实现或重复的概念说明。

公开常量记录完整导入路径、当前字面值和一句话用途。常量既然由模块 `__all__` 显式导出，其当前字面值就是对应 SDK wheel 版本公共 API 的一部分；未导出的内部常量不进入文档。

错误部分只列调用者能够识别和处理的公开异常类型及其稳定触发条件。异常消息原文不构成文档合同；生成器不枚举 Python、文件系统或第三方库理论上可能产生的全部异常。只有源码明确转换、测试覆盖或真实公共消费代码依赖的错误才进入 reference。

## 公共 API 发现规则

公共 API 只从对应 wheel 明确支持的公开导入入口生成：

- 框架侧以 `kat.__all__`、`kat.dataprovider.__all__` 为 `base-api.md` 的公共 API 根。
- SDK 侧只记录 `kat_sdk` 明确公开的 Python 模块，并以各模块自己的 `__all__` 为 `api.md` 和 `reference/` 的公共 API 根。Provider/Workflow 的 KAT declaration 与 inspection 身份仍按现有 SDK 发现合同处理，不从可导入符号反向猜测。
- 明确登记的 SDK 模块进入 `api.md`；模块 `__all__` 导出的函数、类、异常和常量进入对应 `reference/`，导出类的公开构造函数、方法与属性也在该 reference 中说明。
- 名称以下划线开头的成员不进入文档。
- 不扫描或记录未由受支持模块 `__all__` 导出、但碰巧能够 import 的内部符号。

这条规则同时定义各 wheel 的文档生成范围和 AI 可依赖的公开能力边界；文档生成不从测试或现有 Markdown 反向猜测额外的公共符号。

## 内容生成规则

对 `_API_MODULES` 登记的 SDK 公共 API，`kat-dev-sdk` 按以下方式形成待审核文档：

- 直接读取 SDK 源码 checkout；不要求先构建、安装或导入 wheel。
- 通过静态源码分析解析 `__all__`、定义、签名、类型标注、默认值和 docstring；生成期间不 import 或执行 SDK 模块。
- 从源码提取公开导入路径、签名、类型标注、默认值、公开成员以及它们之间的结构关系。
- 输出签名保留源码中的参数顺序、参数种类、默认值和返回类型，但递归展开以下划线命名的私有类型别名。例如源码中的 `_PathLike` 在文档中显示为 `str | os.PathLike[str]`。
- 若类型标注依赖无法静态解析或展开的私有名称，按证据不足处理并停止生成，不能把该私有名称写入公开文档。
- 综合 docstring、实现和测试生成一句话用途，以及 `reference/` 中的详细合同与必要示例。
- 示例只从已有测试或真实消费代码提炼，并收敛为展示该公共模块的最小调用；没有可验证用法时不生成示例。
- docstring 缺失或不完整时仍可依据实现和测试生成草稿，不要求开发者先把相同内容复制进 docstring。
- 人工审核生成结果是否准确表达实现、测试和预期公共合同；未经审核的生成结果不作为正式 API 文档。
- 任一 `__all__` 符号若缺少足够源码、测试或真实消费证据，无法可靠生成用途与公共合同时，`kat-dev-sdk` 必须列出该符号并停止生成正式文档。
- 不得静默遗漏已导出的符号，不得推测无证据的行为，也不得用“待补充”等占位内容绕过生成失败。补足证据后重新生成并进入人工审核。

测试和现有 Markdown 可以为已由 `__all__` 确认的 API 提供行为证据，但不能据此扩大公共符号集合。

SDK 版本从 wheel 使用的同一源码版本源读取，使生成文档可以绑定未来由该 checkout 构建的 wheel。已构建或已安装 wheel 不参与正文生成；用实际 wheel 做一致性检查属于后续发布校验问题。

静态分析使生成过程不依赖原生扩展已经构建，并避免模块导入副作用。无法由源码静态确定的信息必须列为生成失败原因并停止正式稿；人工审核只能核对已有证据或要求补足证据，不能通过执行 SDK 或主观猜测补写合同。

## Workflow 开发时的使用流程

`kat-author` skill 负责在创建或修改 Workflow 时消费公共能力文档。`kat-dev-sdk` 只负责 SDK 开发及 SDK 文档生成，不执行 Workflow 作者流程。

新增 Workflow，或修改已有 Workflow 的行为、输入、输出、依赖及数据处理步骤时必须执行本流程。只修改文案、格式，或只调整不改变能力的测试时无需重复执行。

`kat-author` 开发新的 Workflow 前必须检查公共能力文档，按以下顺序渐进读取：

1. 按现有 `references/reuse-check.md` 开始开发前复用检查。
2. 读取当前提供的 `base-api.md`，确认已有框架作者能力。
3. 完整读取当前提供的短 `api.md`，按一句话用途筛选可能复用的 SDK Python 模块。
4. 只读取候选模块对应的 `reference/<module>.md`，核对输入、输出、错误、边界和经验证示例。
5. 确认适用后再在 Workflow 中使用其公开导入路径；未命中的 reference 不加载。
6. SDK Workflow 与 Provider 继续通过现有 `kat inspect workflow`、`kat inspect provider` 核对当前安装版本的 declaration、参数和 Guide；生成的 API 文档不替代 inspection。

开始实现前，`kat-author` 必须记录本次复用结论：列出选中的 `kat_sdk.<module>.<symbol>`、它满足需求的原因和对应 reference；没有匹配项时记录实际检查过的文档、inspection 范围及具体能力缺口。该记录用于证明实现选择经过公共能力核对，不要求复制 reference 正文。

如果 `api.md` 与 inspection 均没有满足需求的能力，`kat-author` 记录已检查的 `base-api.md`、SDK `api.md`、实际读取的 reference 和 inspection 范围，不再扫描 SDK 源码寻找未公开符号，也不调用未进入公共文档的内部 API。当前任务需要的领域逻辑按最小切片留在 PACK 内，使用框架公开 API 实现；若缺口应成为跨 PACK 复用的官方能力，则转交 `kat-dev-sdk`，不由 `kat-author` 直接修改 SDK。

如果 `base-api.md`、SDK `api.md` 或候选 reference 不可取得，`kat-author` 停止依赖该复用判断的实现并报告文档缺项。不能从 SDK 源码猜测，也不能把“文档不存在”解释为“没有公共能力”；取得文档后再继续。

`kat-author` 不读取 distribution metadata，也不比较文档与 wheel 的版本号；它默认信任当前部署提供了正确配对的文档。版本绑定由文档交付侧保证，不属于作者使用流程。

该流程用于在写代码前快速判断能否复用公共能力，避免把全部详细文档一次性放入上下文。

最新主线已经提供 `kat-author`，其 `references/reuse-check.md` 是本流程的唯一接入点；总入口 `kat` 也已将 PACK、Provider 与 Workflow 创作任务路由到该 Skill。本文修改这条既有流程，不创建平行作者入口或第二套复用检查。

## 文档维护分工

`kat-dev-sdk` skill 负责在 `kat/sdk/docs/` 生成 SDK 的 `api.md` 与 `reference/`，人工负责审核文档是否准确、精简并与 SDK 公共 API 一致。用户自行把审核后的文件复制到 `kat-author/references/` 供作者流程使用。wheel 构建只产生 wheel，不将这些 API 文档装入 wheel，也不承担文档生成或复制。框架侧流程独立拥有 `base-api.md`，`kat-dev-sdk` 将其作为生成和开发时的只读输入。

版本绑定用于明确每份文档描述的是哪一个 wheel 版本。具体发布目录与维护机制不在当前生成设计范围内。

最新主线已经提供 `kat-dev-sdk`，其源码位于 `kat/skills/kat-dev-sdk/`，并明确维护 `kat/sdk/`。本文在该既有 Skill 中增加新的文档生成职责，不创建同名平行 Skill。

## 针对 PR #297 的修改判断

[PR #297](https://github.com/maokelong/kat-cli/pull/297) 已经建立独立 `kat-sdk` wheel、Provider/Workflow 发现、随包 Guide、`kat-dev-sdk` 和 `kat-author`。这些能力继续保留。本方案只替换其中 API 文档的生成、交付和作者侧使用方式，不整体回退该 PR。

当前主线的 API 文档链路与本方案存在五个实质冲突：

1. `kat/sdk/sdk_build.py` 在 setuptools `build_py` 阶段递归扫描 `providers/` 和 `workflows/`，将 Griffe2MD 输出写入临时 `build_lib/kat_sdk/knowledge/**/*.api.md`。
2. `kat/sdk/pyproject.toml` 把 `knowledge/**/*.md` 作为 package data，因此这些生成文件进入 wheel。
3. 生成范围按文件位置推断，不以明确登记的公共模块及其 `__all__` 为边界，并且完全遗漏 helpers 公共 API。
4. 文档只根据签名和 docstring 渲染，未按本方案综合实现、测试和真实消费者证据，也没有进入人工审核后再交付的流程。
5. `kat-author` 当前从 `kat/references/helpers|providers|workflows` 和 SDK 源码寻找能力，尚未消费自身目录中的 `base-api.md`、`api.md` 与按需 reference。

需要严格区分三类内容：

- **SDK API 目录**：`kat/sdk/docs/api.md` 与 `kat/sdk/docs/reference/`，供 Workflow 作者判断可复用的 Python API；从源码生成、人工审核，不进入 wheel。
- **Runtime Guide**：`kat/sdk/knowledge/providers/` 与 `kat/sdk/knowledge/workflows/` 中由 declaration 的 `guide=` 直接引用的文件，供安装后的 Provider/Workflow inspection 读取；继续进入 wheel，不需要目录首页。
- **作者侧快照**：用户审核后手工复制到 `kat/skills/kat-author/references/api.md` 与 `reference/` 的文件；`kat-author` 只读此处。`base-api.md` 同样位于作者侧，但由框架开发流程拥有。

### 公共模块的唯一登记根

当前 `kat_sdk.PROVIDER_MODULES` 只服务 Runtime 的 Provider 发现，不能充当完整 Python API 清单。实现时在 `kat/sdk/__init__.py` 增加一个独立、私有且有序的 `_API_MODULES` 字面量元组；它只登记可导入的公共 Python 模块，不改变 Runtime discovery：

```python
_API_MODULES = (
    "kat_sdk.providers.ftrace",
    "kat_sdk.providers.trace_streamer",
    "kat_sdk.helpers.demo.greeting",
)
```

`kat-dev-sdk` 静态读取该元组，并按元组顺序生成 `api.md`。每个登记模块必须以字面量 `__all__` 定义公开符号及顺序；模块缺失、重复登记、无法静态读取 `__all__`、导出名不存在或证据不足均停止正式文档生成。未登记模块和未导出符号均视为内部实现。

Workflow 模块不进入 `_API_MODULES`。Workflow 的公共身份来自 `@kat.workflow` declaration、PACK inspection 与关联 Guide，而不是可导入函数；不为它增加第二套 Python API 文档。Provider 同时具有两个互补视图：Provider 类作为可导入 Python API 进入 reference，来源关系、Schema、外部工具及查询语义继续由 inspection 返回的 Guide 说明。

## 明确修改方案

### 1. 将 API 生成从 wheel 构建中拆出

| 文件 | 修改 |
| --- | --- |
| `kat/sdk/sdk_build.py` | 删除 `render_object_docs` 及写入 `*.api.md` 的逻辑；将 `generate_knowledge()` 收敛为只读的 `validate_knowledge()`。继续静态检查 Provider/Workflow declaration 引用的 Guide 以及 `knowledge/` 内本地链接，不 import 或执行 SDK 模块；构建不得创建或修改 `docs/` 或任何 `*.api.md`。 |
| `kat/sdk/pyproject.toml` | 删除只用于 Markdown 渲染的 `griffe2md` 构建依赖；保留 Guide 静态校验仍需的解析和 Markdown 依赖。继续只把 `pack.toml`、代码、Workflow 入口和 `knowledge/**/*.md` Runtime Guide 纳入 package data，不加入 `docs/`。 |
| `kat/sdk/MANIFEST.in` | 继续包含构建校验器、源码和 `knowledge` Guide；不新增 `docs/`。 |
| `kat/sdk/knowledge/index.md` | 删除。Runtime 不读取该文件；Provider/Workflow inspection 直接从 declaration 的 `guide=` 定位具体 Guide。移除旧的 `*.api.md` 导航后，首页没有独立职责。 |
| `kat/sdk/knowledge/workflows/demo/greeting.md` | 删除指向 `greeting.api.md` 的链接及构建生成说明；保留 Workflow 分析 Guide 本身。 |
| `build/build_sdk_wheel.py` | 从 wheel 必需资源集合移除 `kat_sdk/knowledge/index.md`；保留版本、文件名、依赖布局、PACK manifest、declaration Guide 非空和纯 Python wheel 校验。新增反向约束，拒绝整个 `kat_sdk/docs/**`、旧布局 `kat_sdk/api.md` 与 `kat_sdk/reference/**`，以及任何 `kat_sdk/knowledge/**/*.api.md`。不要求 API 文档存在，也不读取其版本。 |
| `build/verify_sdk_install.py` | 删除 fixture 对 `knowledge/index.md` 的修改，以及安装验收中对首页和 `*.api.md` 的链接、生成和存在性断言；增加安装后没有上述 API 文件的断言。保留 declaration 直接定位 Provider/Workflow Guide、实际查询与执行、跨 PACK 调用、升级清理和卸载回归。 |
| `.github/workflows/prepare-payload-ci.yml` | 将“安装 SDK documentation build dependencies”改为安装 Guide 静态校验依赖；不增加 API 文档生成或复制步骤。`sdk-ci.yml` 与 `full-ci.yml` 继续只构建并安装同一个 wheel 候选。 |

`knowledge/index.md` 可以删除，但 `knowledge/` 不能整体删除。框架 Runtime 会从已安装 `kat_sdk` 中直接读取 declaration 关联的 Guide，API 文档移出 wheel 不改变这条运行时合同。

### 2. 让 `kat-dev-sdk` 生成并审核源码侧文档

| 文件 | 修改 |
| --- | --- |
| `kat/sdk/__init__.py` | 新增独立 `_API_MODULES` 有序清单；保留 `PROVIDER_MODULES` 的现有 Runtime 职责，二者不合并。 |
| `kat/sdk/docs/api.md`、`kat/sdk/docs/reference/` | 按本文已经确认的模板生成首份 SDK API 目录；Provider 类和 helper 函数按 `_API_MODULES` 与各模块 `__all__` 完整覆盖。文件先留在 SDK 源码树中等待人工审核。 |
| `kat/skills/kat-dev-sdk/SKILL.md` | 将“同步实现和旧的三类 kat 导航”改为“源码侧复用检查、生成 API 文档、人工审核、再独立构建 wheel”。明确 Skill 只写 SDK 源码目录，不自动写 `kat-author`。 |
| `kat/skills/kat-dev-sdk/references/sdk-development.md` | 目录树增加 `docs/api.md` 与 `docs/reference/`，从 `knowledge/` 布局移除 `index.md`；用本文的静态生成、证据、失败和审核规则替换构建期 `knowledge/*.api.md` 规则。开发前只读作者侧 `base-api.md` 核对框架能力，但不生成或修改它。 |
| `kat/skills/kat-dev-sdk/references/sdk-build.md` | 将构建职责收敛为 wheel、SHA256、declaration 关联的 Runtime Guide 与安装升级验收；从 wheel 结构和检查步骤删除 `knowledge/index.md`，并明确 API 文档不由构建生成、复制或打包。 |

SDK 开发不能继续复用 `kat-author/references/reuse-check.md`：SDK 开发需要检查 SDK 源码，而领域作者被明确禁止回查 SDK 源码。`kat-dev-sdk` 在自己的开发指南中维护源码侧复用检查；`reuse-check.md` 仍是 `kat-author` 唯一的作者侧接入点。

### 3. 在审核完成后切换 `kat-author`

| 文件 | 修改 |
| --- | --- |
| `kat/skills/kat-author/references/base-api.md` | 由框架开发流程交付首份审核后的 Pack Authoring API 文档。启用新的 hard-stop 规则前该文件必须存在。 |
| `kat/skills/kat-author/references/api.md`、`reference/` | 由用户将已审核的 `kat/sdk/docs/api.md` 与整个 `kat/sdk/docs/reference/` 手工复制到此处；代码、构建脚本和 Skill 装配均不得代替这一步。 |
| `kat/skills/kat-author/references/reuse-check.md` | 重写为固定读取顺序：完整读取 `base-api.md`，完整读取短 `api.md`，只读取候选 reference，再对 SDK Workflow/Provider 执行 list/detail inspection，并检查当前 PACK 自身能力。记录选中符号及原因，或记录实际检查范围与能力缺口；记录放在当前开发任务的实现前说明和最终交付说明中，不新增一份仓库状态文件。缺少任一判断所需文件时停止依赖该判断的实现，不扫描 SDK 源码、不使用内部 API，也不以 runtime signature/docstring 或其他版本文档兜底。 |
| `kat/skills/kat-author/SKILL.md` | 明确新增 Workflow，或修改行为、输入、输出、依赖和数据处理步骤时必须执行复用检查；纯文案、格式和不改变能力的测试修改可以跳过。 |
| `kat/skills/kat-author/references/pack-authoring-flow.md` | 将旧的 helpers/providers/workflows 静态导航和硬编码 Provider 构造合同改为指向 `reuse-check.md`；保留当前 PACK inspection、公共 Provider/Workflow inspection 和真实调用验证。具体 Python 构造合同归 SDK reference，来源语义归 Provider Guide。 |

新的作者流程不能先于文档启用。实现切片必须先准备并审核 `base-api.md` 与 SDK 源码文档，待用户完成手工复制后，再合入依赖这些文件的 `kat-author` hard-stop 规则。

### 4. 清理 PR #297 留下的平行事实来源

`kat/skills/kat/references/helpers/`、`providers/`、`workflows/` 不再作为作者复用判断或 SDK 开发交付要求。首个修正 PR 至少要解除 `kat-author`、`kat-dev-sdk`、README 和命令合同对这些目录的 API 权威引用；完成内容迁移后删除这三套静态导航，避免与 SDK reference、inspection 和 Guide 同时陈述同一合同。

迁移规则如下：

- helper 的导入路径、签名、异常和已验证示例进入 SDK reference。
- Provider 的 Python 构造与调用进入 SDK reference；来源 relation、Schema、限制和诊断进入 declaration 关联的 Guide。
- Workflow 的名称、参数和 Guide 由 inspection 提供，不复制成 Python API reference。
- PACK 自己的 `helpers/`、`knowledge/helpers/` 和文档继续由 PACK 拥有，不受此迁移影响。

同步修改：

- `kat/skills/kat/SKILL.md`：PACK 开发和复用判断路由到 `kat-author`；当前 Workflow/Provider 的运行时能力通过 inspection 获取。
- `kat/skills/kat/references/command-reference.md`：删除 helpers 导航和通过 `importlib.resources` 查找随 wheel API 的说明，保留 Provider/Workflow inspection 命令合同。
- `README.md`、`CONTEXT.md`、`kat/sdk/README.md`：改正 API 文档归属与 wheel 边界，并把审核后的生成文档定义为受版本控制的交付源码而非普通 build artifact；`CONTEXT.md` 同时明确作者侧 API 快照是公共 API 发现入口的受控例外，Provider/Workflow Guide 仍随其 SDK 所有者交付，不写生成脚本细节。
- `docs/specs/public-capability-sdk.md`：在顶部标明 API 文档章节由本文局部替代，保留 #297 已完成的 SDK 架构与历史验证记录。
- [ADR-0086](../adr/0086-api-documents-are-reviewed-author-references.md)：局部替代 ADR-0085 中“Griffe/Griffe2MD 构建期生成 API 并随 wheel 交付”的决定；ADR-0085 的独立 SDK、发现和 Guide 决定继续有效。

### 5. 重写验证而不是删除验证

| 验证 | 新的断言 |
| --- | --- |
| `build/tests/test_sdk_build.py` | Guide 校验静态读取且不 import 模块；缺 Provider Guide 失败；坏链接和越界链接测试改为写入一个实际 Guide，不再依赖 `knowledge/index.md`；校验前后 SDK 文件集合与字节不变；不会生成 `*.api.md`。删除“生成中文 API”和“生成文件覆盖手写文件”测试。 |
| `build/tests/test_wheel_artifacts.py` 及 SDK archive tests | 有效伪 wheel 不再创建 `knowledge/index.md`；真实或伪造 wheel 分别注入 `kat_sdk/docs/api.md`、`kat_sdk/docs/reference/...`、旧布局 `kat_sdk/api.md`，以及 `knowledge/**/*.api.md` 时必须被拒绝。正常 wheel 仍包含代码、manifest 和 declaration 引用的 Guide。 |
| API 文档审核 | `_API_MODULES` 顺序与 `api.md` 一致；每个登记模块恰有一个 reference；每个 reference 的符号顺序与字面量 `__all__` 一致；无额外 reference；全部相对链接有效；私有别名无法展开或证据不足时没有正式稿。 |
| Runtime 回归 | Provider/Workflow list/detail 仍返回真实 declaration 与非空 Guide；缺 Guide 明确失败；Provider import/query、Workflow 直接执行与 `ctx.run()`、安装、升级、卸载及 Windows/Linux CI 保持通过。 |
| 作者侧装配 | 用户复制后，Skills 集合包含 `base-api.md`、`api.md` 和完整 `reference/`；归档重定位后相对链接可读；`kat-author` 规则明确读取顺序、缺失停止及复用记录。 |
| 产物隔离 | wheel 构建前后，源码中的 `kat/sdk/docs/` 文件集合和字节完全不变；wheel 内没有 API 文档，但 Runtime Guide 完整。 |

## 实施切片

1. **定义 API 面并生成源码文档**：加入 `_API_MODULES`，补齐登记模块的字面量 `__all__`，由 `kat-dev-sdk` 生成 `kat/sdk/docs/api.md` 与 `docs/reference/`，完成人工审核。此时不改变 wheel 或作者流程。
2. **修正 wheel 边界**：移除构建期 API 渲染和无消费者的 `knowledge/index.md`，保留 declaration Guide 与链接校验，改写 wheel 与安装测试，证明 API 文档没有进入 wheel且 Runtime inspection 不回归。
3. **准备作者侧文档**：框架流程交付 `base-api.md`；用户手工复制已审核的 SDK 文档到 `kat-author/references/`。该步骤没有自动同步或版本比对。
4. **切换作者使用流程**：重写 `reuse-check.md` 和 Pack authoring 文档，解除旧三套导航的权威地位，并更新总路由、README、CONTEXT、SDK SDD 与 ADR。

这四个切片可以位于一个后续 PR，但提交顺序必须保持上述依赖，保证任何启用 hard-stop 的提交都同时拥有可读取的作者侧文档。实现基于包含 #297 的最新主线，不整体回退其独立 SDK、discovery、inspection 和 Runtime Guide 能力。

## 明确不改

- 不修改 CLI、PACK discovery、Provider/Workflow declaration 或 inspection 协议。
- 不删除随 wheel 交付的 Provider/Workflow Guide。
- 不让 wheel 构建生成、审核、复制或打包 API 文档。
- 不增加文档与 wheel 的 runtime 版本比较；使用方默认信任交付配对。
- 不实现手工复制的自动同步、漂移 CI、修订号、历史目录或独立文档制品发布。
- 不把 Workflow 函数加入 Python API reference，也不新增第二套 Workflow 静态发现规则。
- 不整体回退 PR #297 已建立的独立 SDK wheel 和真实安装升级验证。

## 现有边界

本次文档体系覆盖框架 wheel 的 Pack Authoring API 和 SDK wheel 的公共能力，分别由 `base-api.md` 与 SDK API 文档说明。AI 分别查阅与两个 wheel 版本匹配的文档，开发 Workflow 前据此判断可复用能力。

本文以 PR #297 已合入的独立 `kat-sdk` 与 `kat-workflow` 为现状，只修改两者 API 文档的生成和作者侧使用边界。独立公共能力包、第三方 SDK 及新的安装升级机制不在本次范围内。

## 暂不展开

- 同一 wheel 版本的文档修订号、历史保留和覆盖策略。
- 文档发布目录、制品命名与发布校验机制。
- 独立公共能力包的安装与升级。
