# 随 Release 发布的 KAT 用户手册

状态：HTML 手册已实现；自动化与运行链路验证已执行，完整用户验收尚有环境缺口，见文末。

交付跟踪：[Issue #290](https://github.com/maokelong/kat-cli/issues/290)。

实施基线为 `a30c776f`。需求收敛后主分支已经合入四个同级 KAT Skills 和内置 Python pip 支持，本文按该代码事实更新：手册源码移至 `kat/skills/kat/user-manual.html`，包内路径仍为 `kat/user-manual.html`；安装与升级覆盖四目录，日常维护补充已实现的依赖管理。不改变用户选定的读者、对话主线、真实数据和离线 HTML 范围。

## 问题与已确认范围

用户拿到 KAT Release 包后，需要通过随包手册了解 KAT 怎样使用。第一版同时完整覆盖分析使用者与 PACK 开发者，不能只提供分析入门后把开发者转交给零散的 Agent reference。

手册面向人阅读，遵循 KAT Skills 作为用户入口的现有产品边界。分析使用与 PACK 开发共用必要的安装和基础概念说明，再分别提供各自的操作路径。

第一版安装与启用指引面向 Claude Code。分析和开发示例均从该工具中的 KAT 使用方式展开。

PACK 开发章节以对话操作为主：教用户向 Claude Code 描述需求、提供输入材料和必要背景，并检查测试证据与交付结果。用户能够完成 PACK 开发任务，不以亲自编写 Python 为前提；完整 Python 教程与逐项 API 参考不作为本手册的主线。

开发示例需要明确用户应提供的信息，例如目标目录、PACK owner、分析问题、输入来源、预期输出与验收标准，并展示创建、修改、测试及诊断任务怎样表达。只有空骨架可被发现，不足以表示一个分析能力已完成；验收应核对真实 Workflow 执行与测试结果。

开发章节覆盖完整交付：本机开发、运行与测试后，把 PACK 分享给同事，并指导对方安装、验证和开始分析。

这项工作解决当前交付缺口：现有文档主要面向仓库开发者和 Agent，拿到 Release 包的用户缺少贯穿安装、分析、开发和分享的说明。第一版以一份随包可读、按实际能力可操作的手册完成这个最小切片。

## 使用自己的 Trace 上手

首次上手直接围绕真实业务数据展开：用户先准备自己的 Trace 和分析目标，再通过 Claude Code 使用 KAT。手册给出可替换为实际信息的对话示例，说明需要提供的文件位置、采集背景及问题范围，并展示如何查看结论、核对证据和继续追问。

分析流程先发现当前可用 PACK 与 Workflow。没有匹配能力时，说明缺少什么，并引导用户明确提出创建或扩展 PACK 的需求后进入开发章节；不能把一次分析请求视为自动修改 PACK 的授权。支持哪些来源和分析问题，以当前实际可用的 Workflow 为准。

## 离线 HTML 交付

手册使用中文离线 HTML，解压后通过浏览器直接打开。阅读正文、目录导航与样式不依赖网络或本地服务器；外部官方文档只作补充参考，核心 KAT 使用步骤必须在手册内说明完整。离线阅读不表示 Claude Code 或用户选择的数据来源能够离线运行。

最小实现方案为直接维护 `kat/skills/kat/user-manual.html`，装配后位于包内 `kat/user-manual.html`。采用单文件 HTML 和内嵌样式，以页内锚点提供目录导航，正文可用浏览器查找；不加载 CDN、远程字体或远程图片。该文件作为唯一手工维护的手册来源，首版不增加 Markdown 到 HTML 的构建链。

手册随所属 KAT Release 一起交付和更新，仓库 README 提供入口。现有装配复制即可带入 HTML，不增加独立的 Release 附件或修改公开资产集合。第一版不另外制作 PDF 或在线文档站。

## Claude Code 安装依据

已核对 [Claude Code 官方 Skills 文档](https://code.claude.com/docs/en/skills)：个人 Skill 放在 `~/.claude/skills/<skill-name>/`，项目 Skill 放在项目根目录的 `.claude/skills/<skill-name>/`；可通过 `/skill-name` 显式调用。

结合当前发布目录，手册指导用户将同一版本的 `kat/`、`kat-analyze/`、`kat-author/`、`kat-review/` 四目录一起放入上述 skills 目录，保持同级，再从 Claude Code 调用 `/kat` 或任务入口。必须保留全部 references、脚本与平台载荷，不能只复制 `SKILL.md`。具体平台步骤与首次调用仍须从实际发布包验证，官方加载规则本身不等于 KAT 安装已通过验收。

安装章节以用户已有可正常使用的 Claude Code 为前提，为其自身安装提供官方链接。KAT 步骤优先讲个人级安装，再简述项目级安装；分别给出 Linux 与 Windows 的路径示例。平台支持范围沿用目标 Release：当前 Linux x86_64 要求 glibc 2.28 及以上，Windows 10/11 x86_64 客户端仍为预发布候选。手册不能据此扩大平台支持承诺。

## PACK 分享与接收

分享对象是完整 PACK 目录，至少保留 `pack.toml`、实际需要的 Workflow、Provider、helper 与 knowledge 文件；为了让接收方执行手册中的验证流程，交付还应包含测试和必要的受控 fixture。将目录整体压缩仅是传输方式，不引入新的 PACK 格式或发布系统。

交付说明写明已验证的 KAT 版本、分析用途、所需输入、外部工具或服务等环境前提，以及如何检查成功。原始业务 Trace、凭据、个人配置及运行生成的结果不默认成为分享内容；真实数据由接收方按自己的场景提供。

接收方可以保留 PACK 在自己的工作目录，向 Claude Code 明确该目录；需要默认发现时，将完整 PACK 放在当前 Data Home 的 `packs/` 直接子目录中。手册解释 `pack.toml` 必须直接位于 PACK 根目录，以及同名 PACK 来自不同目录时会冲突。底层精确目录选择由 Agent 使用已有 `--pack-dir` 完成，不虚构尚未实现的 `kat install` 等命令。

接收完成后，通过 Claude Code 检查 PACK 和 Workflow、运行 PACK 测试，并用自己的输入完成一次分析。若 Workflow 调用其他 PACK 或使用额外解析器、数据服务，交付说明必须列出已验证的运行前提；PACK 目录复制成功不等于这些依赖已经满足。没有测试、测试失败或实际运行受阻时，不能宣称接收验收通过。

## 章节与完成标准

| 章节 | 用户需要完成的任务 | 手册必须说明的可观察结果 |
| --- | --- | --- |
| 开始之前 | 了解 KAT 能做什么，选择分析或开发路径 | 区分 KAT Skill、PACK、Workflow；知道需要提供自己的数据与目标 |
| 安装与首次启用 | 获取完整 Release 包，安装到 Claude Code，调用 `/kat` | Skill 可被调用，平台载荷可用，当前 PACK 能力可被发现；空列表有明确说明 |
| 准备真实分析 | 提供 Trace 位置、采集背景、业务现象和关注范围，必要时提供已有 PACK 位置 | Claude 能确认可用 Workflow 与所缺输入；无匹配能力时明确停点和下一步 |
| 完成一次分析 | 发起分析、阅读报告、核对关键证据 | 直接结论、使用的 PACK/Workflow、可追溯的 Session/Run 身份与证据、适用范围和不确定性 |
| 继续分析与复核 | 在当前对话追问、新对话中继续或调用已发布的复核入口 | 知道哪些 Session/Run 信息需要保存和提供，区分读取已有证据、新增执行和事后复核 |
| 创建和维护 PACK | 通过对话创建骨架、补充实际能力、修改已有能力、诊断失败 | 目标目录和 owner 明确；变更范围、输入与输出约定可检查；骨架和完整能力有区别 |
| 测试与验收 | 要求 Claude 检查声明、运行测试并执行真实 Workflow | 返回实际 inspection、测试和运行证据，失败原因与未完成部分清楚 |
| 分享与接收 PACK | 准备完整目录及说明，交给同事，由对方放置、检查和使用 | 接收方能发现、测试和执行；目录、名称冲突及环境缺项有排查路径 |
| 日常维护与排错 | 理解数据位置、依赖管理、整体更新 KAT、处理常见失败、按需清理分析 | 区分 Skills 安装目录与 Data Home；说明共享 Python 环境、配置和 Session 删除的实际影响，不承诺跨版本迁移 |

每个主要操作提供可复制并替换实际信息的自然语言示例，随后解释应看到什么，以及失败时应补充什么。对话以任务目标组织；CLI 命令只在必要的安装、自检或排错位置出现，不成为日常使用主线。公开说明遵循当前结果契约，不展示伪造的业务结论或未实际运行的成功记录。

## 非目标与方案取舍

- 不改变 KAT CLI、Runtime、PACK 发现或现有 Skill 执行行为，不新增分析能力。
- 不新增默认业务 PACK、教学数据集、PACK registry、安装器或升级器。
- 不扩展到其他 AI 工具，不编写完整 Python/API 教程，不发布 PDF、在线站点或独立 Release 文档资产。
- 选择离线 HTML，是为了让用户解压后直接通过浏览器阅读；直接维护单文件，避免新增文档构建链和多份人工内容。
- 复用现有整树装配，保持归档和 SHA-256 两项公开资产合同。手册格式与导航是可逆的文档实现选择，不新增 ADR 或领域术语。

## 已核对的交付基础

- 公开安装资产为 `kat-skill-<version>.tar.gz` 及其 SHA-256 校验文件。
- `build/assemble_skill.py` 将 `kat/skills/` 中四个同级 Skill 复制到集合根，再把载荷与 Bundled PACK 放在其中的 `kat/`。手册位于源码的 `kat/skills/kat/`，无需新增装配参数。
- 仓库根 `README.md` 当前不进入发布包；`kat/skills/kat/SKILL.md` 和其 references 主要指导 Agent 执行任务。
- 工作区中有尚未实施的公共能力包设计；用户手册必须按目标 Release 的实际能力编写，不能把设计目标直接写成可用操作。

当前基线的 `kat/packs/` 只有 `.gitkeep`，不能假定用户开箱即拥有原先的 CPU 时间或关键路径分析 PACK。已有 `kat-author/references/examples/dataprovider-pack/` 位于 reference 目录，需要显式提供 PACK 位置才能发现；其小样例不作为首次上手主线。

## 最小交付与预计文件

一个 PR 完成手册、入口及随包验证，不拆成没有操作内容的空壳页面：

| 文件 | 变更 |
| --- | --- |
| `kat/skills/kat/user-manual.html` | 新增完整中文手册、内嵌样式与页内目录 |
| `README.md` | 增加用户手册入口及随包位置说明 |
| `build/tests/test_assemble_skill.py` | 最小补充真实 Skill 源手册在装配结果中的交付检查，保持装配器的黑盒复制合同 |
| `docs/specs/release-user-manual.md` | 维护本规格和必要的实施边界修订 |

默认不修改装配脚本、生成的 Release workflow、`AGENTS.md`、`CONTEXT.md` 或已有 ADR。实现中发现无关旧文档问题时另行记录，不扩大本切片。

## 验证计划与证据

1. **入包**：补充并运行 `python -I -B -m unittest discover -s build/tests -p "test_assemble_skill.py"`，用真实 Skill 源与隔离的测试载荷确认 HTML 被完整复制。最终还要列出并解压本次候选归档，确认 `kat/user-manual.html` 实际存在；装配单测不替代最终归档检查。
2. **离线阅读**：从解压位置通过浏览器直接打开 HTML，检查中文、目录锚点、窄窗口和对话示例阅读；检查外部资源加载和本地链接，确认核心内容不依赖网络。若无法进行真实离线浏览器验收，明确记录未验证，不能用源码检查替代。
3. **Claude Code 启用**：按手册从完整包安装并调用 `/kat`，记录 KAT 版本、Claude Code 版本、主机平台、安装位置与可观察结果。Linux 和 Windows 安装说明分别验证；Windows 候选环境的成功不提升为正式支持。
4. **真实分析**：选取可访问的真实 Trace、明确问题及相匹配的 PACK，按手册执行并取得可追溯结果。记录输入身份、操作、Session/Run 身份与必要结果摘要；原始业务数据不纳入手册或 PR。只有通用提示词、合成小样例或“没有匹配 PACK”的演示，不能证明真实分析成功路径通过。
5. **开发与分享**：在隔离的用户工作目录通过对话创建或扩展一个能处理上述问题的 PACK，核对声明、测试与实际运行证据；把交付目录复制到第二个独立接收环境，移除对原开发目录的依赖后再次发现、测试和分析。可在同一台主机的隔离环境执行，但不得表述为跨机器或跨平台已验证。

上述是实施验收计划。实际执行结果与未完成项单独记录；没有相应验证的步骤不能标为已通过。

## 维护与实施前核对

手册是同版本交付内容。改动安装、输入、结果、PACK 分享或用户可见故障行为的 PR，应同步更新受影响章节，由该变更的作者和 reviewer 核对；不另建文档版本矩阵或独立发布流程。

实施时以选定目标提交及完整 Release 产物为事实基线，重新核对已安装公共能力和 PACK。工作区中尚未实施的规格不作为用户可用能力；若默认没有业务 Workflow，应如实走能力发现或开发路径。

真实 Trace、适配 PACK、可运行的完整 KAT 包与 Claude Code 环境属于验收前提。先复用已有可访问材料；不足时再针对缺项补充。保留原文件，不把个人路径、业务数据或解析产物写进用户手册。

## 实际验证记录（2026-09-18）

### 交付检查

- 新增装配测试先因真实源目录缺少 HTML 而失败，加入手册后通过；没有修改装配器。
- Windows：`python -I -B -m unittest discover -s build/tests -p 'test_*.py'` 报告 82 项，结果 OK，5 项因 Windows 符号链接权限（WinError 1314）跳过。
- Linux：WSL 原生 `/tmp` 下单跑 `test_assemble_skill.py`，14 项全部通过，包括 Windows 跳过的 5 项。
- Linux 全套构建测试尝试报告 63 项：60 项通过，3 个测试模块因缺少 `pefile` 无法导入；系统 Python 缺少 `ensurepip`，隔离 venv 也未能建立。没有将 Linux 全套标为通过；相应完整套件已在 Windows 执行。
- HTML 静态检查：标签配对、唯一 ID、14 个锚点、22 个链接（含本地引用）有效；21 个示例块，没有脚本、远程加载元素、CSS 外部导入或外部字体。此检查不替代浏览器显示验收。
- 最终本地候选归档：228,382,026 bytes，SHA-256 `de2710f3a67967bde0f376270348fc530c964f8cd756de0da9887bdefae16a34`，顶层精确为四个 Skills。Linux 原生文件系统完整解压到隔离项目的 `.claude/skills/` 后，`build/verify_skill_collection.py` 通过；`kat/user-manual.html` 与源码逐字节相同（39,061 bytes，SHA-256 `4b5fd30ead0230312af6bab88514883199eed9d9850045d93a7cf3bf9a533df2`）。此候选未发布，载荷来源与限制见下节。
- `git diff --check` 通过。
- 独立规范审查与规格审查均未发现需要修改的问题；审查保留下述环境验收缺口，不据此宣称完整验收通过。

### 真实输入、执行与接收

运行使用经过 SHA-256 校验的官方 `kat/0.1.1-rc.11` 完整载荷，归档 SHA-256 为 `f0dd68b94e0ed4c3ca6d061b336caf2d3972c9a88abb80098da34e586bf64ed1`。用当前装配器组合当前四 Skills 与未修改的官方双平台载荷。`kat/platform` 和 `Cargo.lock` 在 rc.11 与实施基线间无差异，但 main 的 pip 构建与依赖锁已变更；因此以下记录不等于最新 main 完整载荷或新增 pip 能力已验收。

验证问题为“真实文本 Ftrace 含多少事件、有哪些事件类型，已有结果中的 sched_switch 数量是多少”。输入为本地既有 `test/kat_complex_20260818.ftrace`，SHA-256 为 `ebc81a90ccee81f8bf70db2f69cac4955a6357e79c252e0034c8424bbf2bc83f`，使用当前 reference PACK `dataprovider-pack` 的 `summarize-ftrace-events`。素材没有新增进发布包。运行显式使用 `clock_domain=unverified_capture_clock` 作为事件计数域标签，不证明真实采集时钟类型，也未做跨时钟比较。

Windows 11 x86_64 客户端（10.0.26200，ProductType=1）及同机 WSL Ubuntu（x86_64，glibc 2.39）分别设置开发方和接收方的独立 Data Home。接收方把完整 PACK 放在自己的 `packs/` 直接子目录，原开发路径移走后，再按默认发现执行；没有依赖原目录或发送方的显式 PACK 路径。

| 环境与角色 | PACK 测试 / 真实 Trace 测试 | Session ID | Run ID |
| --- | --- | --- | --- |
| Windows 开发方 | 38 / 1 通过 | `01a0b2b8-45fb-76e2-86b5-be24073cffee` | `01a0b2b8-4641-7df3-bcb4-c13a3333bfa0` |
| Windows 接收方 | 38 / 1 通过 | `01a0b2b9-32e6-7a40-9dc9-00f67a1b509f` | `01a0b2b9-3328-7521-aaa5-f138ab695013` |
| Linux 开发方 | 38 / 1 通过 | `01a0b2b8-2e2d-7a33-8a4e-948614cb57ba` | `01a0b2b8-2e3a-72e0-a6d6-4aee08b2c8eb` |
| Linux 接收方 | 38 / 1 通过 | `01a0b2b8-cba3-7f11-adaa-ee32c2650546` | `01a0b2b8-cbb1-7910-bffe-07fec3d5280e` |

四组均完成 manifest、Workflow 和 Provider inspection、测试、Session 创建、真实 Workflow 执行与 Output Query。结果一致：44,344 个事件、25 个类型；追问得到 `sched_switch=3975`，前后 Session 清单一致，没有新增 Run。真实测试另核对 4 个 CPU 和首个事件字段。只有退出码为 0 且结构化响应成功的操作记为通过；这些计数不用于推断业务性能原因。

### 尚未完成的用户验收与既有限制

- **离线浏览器阅读未验证**：浏览器工具的 URL 安全策略拒绝本地 `file://` 页面；未改用其他渲染通道规避限制。中文显示、窄窗口、实际点击导航和复制示例仍需人工浏览器验收。
- **Claude Code 对话链未验证**：隔离运行官方 Claude Code 2.1.276（Node 24.19.0），认证检查返回 `loggedIn=false`。未登录或读取凭据，尚未验证四 Skills 的实际加载、`/kat` 调用及通过对话创建或扩展 PACK。上述现有 PACK 的底层运行和分享验证不能替代此项。
- **Windows 原生安装受阻**：按手册用 Windows `tar -xzf` 解压到隔离 Skills 目录，7 个 Linux Python 符号链接创建失败，退出码 1；没有把不完整解压判为安装成功。手册已说明链接环境要求和失败处理。Windows 运行链使用完整保留的官方载荷通过，不代表原生解包或干净客户端正式验收通过。
- **WSL 文件系统限制**：最初将 Data Home 放在 `/mnt/d` DrvFS 时，生产 Run 发布遇到 `rename-no-replace` 的 `EINVAL`；改为隔离的 Linux 原生 `/var/tmp` 后通过。未修改运行代码或用户正常配置，不声称 DrvFS 已验证支持。
- **环境边界**：两种平台运行于同一主机及其 WSL，不是两台独立机器。新 pip 载荷的安装能力本轮未验证。完整验收仍待上述环境缺口补齐，不据本次记录关闭 Issue。
