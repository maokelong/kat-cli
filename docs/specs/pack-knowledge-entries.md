# PACK 知识入口：纯知识与知识驱动取证

状态：CLI、Session 知识材料与产品 Skill 已实施，公开 CLI 与隔离 AI 任务验收已执行；必需的 rust_diagnostics 门禁已补齐，双平台 CI 正在复验。架构决定见 [ADR-0088](../adr/0088-pack-owned-knowledge-entries.md)。证据与限制见下文。

实施跟踪：[Issue #312](https://github.com/maokelong/kat-cli/issues/312)。Issue 包含可独立阅读的轻量 SDD 与验收清单。

## 问题

报告模板、排查方法、判断准则和评审清单等经验不一定对应有意义的 Python 程序。现有 Workflow 的 Guide 依附程序入口，分析 Skill 又要求先选择并执行 Workflow，导致这类经验无法作为独立能力被选择和使用。

给经验配一个空 Python 函数会制造没有实际程序工作的 Run；仅增加普通文档又不能解决任务入口的发现、选用和恢复。

## 当前事实与设计基线

实现检查基于 `origin/main` 的 `291e29768f844a27b52b2d450f53d2f203ab2c27`。当前工作区位于旧任务分支且有既存未提交修改，本文不将本地旧实现等同于最新主线。

- PACK 通过静态 `pack.toml` 发现，边界由维护方和发布责任决定。没有 `workflows/` 可以得到完整的零 Workflow 列表。
- Workflow 保持 Python 程序入口；无输出 Workflow 仍是成功发布的程序执行，不是知识应用的身份。
- 最新 `kat-analyze` 要求先发现和执行 Workflow，通用工具只用于满足 Workflow 输入合同，不能直接替代分析。
- 最新 Analysis Record 已允许空节点集合，并独立保存目标与总报告正文，因此无 Run 的结果可以复用现有记录模型。
- 当前材料类型只有查询材料，不能据此宣称已支持知识入口及其引用的快照。
- 主线 Run Guide 已由程序捕获并随 Run 保存。知识快照归属 Session，与 Run Guide 保留各自职责。

## 已确定的目标模型

1. Knowledge entry 是 PACK 自有、可独立发现和选用的任务入口，由 AI 应用经验完成任务。
2. PACK 可以只有知识入口，也可以同时拥有知识入口与 Workflow；沿用同一安装和发布边界。
3. 入口采用 Skill 风格 Markdown，以 `name`、`description` 提供选择信息，正文承载方法、边界和交付要求，按需引用其他知识材料。
4. 发现阶段提供名称和描述；选中后读取正文，随后只读取任务需要的引用。
5. 知识入口可以使用任务已授权的通用工具，也可以按需选择已有 Workflow，根据结果继续判断。入口本身不扩大用户授权。
6. Workflow 保持确定性程序执行，AI 判断发生在各次执行之外；应用知识不创建 Run，只有实际执行 Workflow 才形成对应的真实 Run。
7. Workflow Guide 解释所属 Workflow 的结果，知识入口指导任务；可以引用同一份经验文档，共享内容不改变解释作用域或程序调用关系。
8. 知识任务沿用 Analysis Session 和 Analysis Record，支持无 Run 的目标、结果保存和跨任务恢复；不为应用知识建立虚构 Run 节点。
9. 保存实际采用的入口正文与已读取并采用的引用材料。恢复默认沿用该知识快照；明确更新方法时采用新版并保存必要说明，原依据缺失不能用当前文件冒充。
10. 面向 Session 选用入口或引用时，KAT 程序读取原文、保存快照，再返回同一份正文；AI 负责选择知识，不抄写原文来替代程序捕获。仅浏览能力不创建任务快照。
11. PACK 可承载性能分析之外的经验知识，领域和用途由 PACK 声明。平台不按性能领域限制知识入口，默认提供的能力仍以性能分析为主。
12. 找不到匹配的知识入口或 Workflow 时，明确报告能力缺口，不自动转入无入口的任务执行；只有用户授权后才进入经验整理与 PACK 创作流程，临时经验不自动成为已发布能力。

## 作者目录布局

每个入口使用 `PACK/skills/<入口名>/SKILL.md`，仍由 KAT 按 PACK 发现和读取。PACK 内可以有多个知识入口，全部随所属 PACK 一起维护和发布。

```text
<pack>/
├── pack.toml
├── skills/
│   └── stall-triage/
│       ├── SKILL.md
│       └── references/       # 可选的入口专用材料
├── knowledge/               # 可与 Workflow Guide 共享的知识
└── workflows/               # 可选的 Python Workflow
```

只创建实际使用的目录。专用引用可与入口相邻，共享经验继续放在 PACK 的 `knowledge/` 中，由入口和相关 Workflow Guide 引用；无需复制成多个正文。入口身份与声明校验见下文接口方案。

## 知识引用范围

首版知识引用通过 Markdown 相对链接表达，以包含该链接的文件所在目录为基准；KAT 读取及保存快照的最终目标必须位于所属 PACK 内。因此入口可引用相邻 `references/`，也可引用 PACK 内共享的 `knowledge/`，共享文档中的下一层引用仍按其自身位置解析并按需读取。

知识引用的 PACK 边界不限制任务中按公开名称调用其他 PACK 的 Workflow。用户资料、源码和网页继续通过已授权工具作为任务材料使用，不通过知识引用读取接口访问 PACK 外文件，也不自动获得知识快照的保存承诺。

读取时验证实际目标归属，拒绝通过路径或链接逃出所属 PACK 的知识引用。公开引用选择器为下文的 `--path`，不会自动遍历链接。

### 首版文件范围

知识入口声明仍为 Markdown；其引用支持 UTF-8 文本，包括 Markdown、JSON/YAML 等文本模板和代码示例。KAT 按原文读取和保存引用，不把示例代码当作程序入口执行，也不因文本格式增加模板渲染、JSON/YAML 解释或代码编译能力。

图片、PDF 和其他二进制文件不进入首版知识读取与快照合同。它们仍可在用户任务授权范围内通过通用工具作为任务材料读取；该读取不自动获得 Session 知识快照的保存承诺。非 UTF-8 内容明确报读取错误，不能静默替换字符后宣称保存了原知识。

## 知识读取与快照职责

能力列表和只读浏览不创建 Session 或快照。首次为任务选用入口或引用时，使用所属 Session 的程序读取与保存路径：读取来源一次，以取得的同一份原文形成持久快照与成功返回内容，避免读取给 AI 后再从可变化的 PACK 文件重新构造当时依据。

同一 Session 已保存的知识默认从快照返回，不重新采用当前 PACK 文件，因此 PACK 更新或移除不影响这些知识的再次读取。采用新版须显式指定 `--update` 并核对记录修订号。

明确更新方法时保留旧快照，新增一份不可变知识快照，以独立材料 ID 区分。结果正文按需注明采用的材料 ID，旧结论的方法依据仍可按其身份读取；不增加独立版本依赖图。

同一入口只有一份已存正文快照时，可以按名称读取；存在多份时必须明确材料 ID，未指定则报告歧义，不按创建时间或当前 PACK 内容代选。AI 根据已保存正文引用的材料 ID 取得相应方法依据。引用文件同样按实际材料身份读取；入口关联多个不同引用文件不等于其正文存在多个版本。

恢复保证已成功保存的知识，不保证当时尚未读取的引用也具有旧版本。例如旧入口指向一份未采用的共享准则，PACK 移除后可恢复入口和旧结果，但无法凭空恢复那份准则；PACK 更新后的文件也不能冒充其旧版本。此类情况明确说明知识缺口，再按已确定的更新方法边界处理。

快照保存未成功时，不返回宣称已选用且可恢复的成功结果；已有完整快照不能被失败写入破坏。保存成功后返回失败属于响应不确定，沿用现有分析恢复原则核对实际记录，不把当前文件重新读取的内容冒充首次返回。

## 公开操作边界

首版采用以下两个明确分开的操作；具体选择器和响应见下文接口方案。

| 操作 | 用途 | 写入边界 |
|---|---|---|
| `kat inspect knowledge` | 发现知识入口、只读浏览当前 PACK 的正文与引用 | 不创建 Session、Analysis Record 或知识快照。 |
| `kat knowledge read --session S` | 在指定 Session 选用入口或引用，返回任务实际采用的原文 | 首次选用由程序读取并保存快照后返回；已保存知识默认读取原快照。 |

任务读取是可能写入 Session 的操作，不混入 `inspect` 的只读语义。只读浏览得到的当前文件不能当作已保存的任务方法依据，也不能替代原 Session 的历史快照；其正文用于能力检查时，不自动变成新的知识选用。

## 首版最小可交付切片

发现与选择、正文和引用读取、知识快照、结果保存与恢复作为一个完整切片交付。同时调整对应的分析入口、作者说明、命令合同及安装产物，使以下两条路径都可实际完成。

### 路径 A：纯知识任务

以评审清单为知识入口，检查用户提供的受控材料：

1. 从 PACK 发现并选择匹配入口，读取正文及实际需要的共享准则。
2. 在同一 Session 保留目标，应用准则检查材料，形成有出处和限制的结果正文。
3. 保存实际采用的知识与结果，全程不执行 Workflow，不产生 Run 或占位报告树节点。
4. 新任务只有 Session ID 时，可以恢复目标、结果与原方法依据。

### 路径 B：知识驱动取证

以症状排查方法为知识入口，使用已存在的受控 Workflow：

1. 读取方法并根据症状选择一个已有 Workflow，按其公开参数合同准备输入。
2. 在当前目标的 Session 中执行并解释真实 Run；读取其自身 Run Guide 和必要证据。
3. 根据实际发现决定是否继续取证，再保存必要解释与最终结果。
4. AI 临时串联的 Run 保留真实身份，不因知识入口获得虚构父 Run 或程序调用边。

这两个例子只作为验收夹具，能力范围不限于清单评审、排查或报告模板。

## 不做什么

- 不引入由 AI 执行的第二种 Workflow、DAG 或程序运行中的模型回调。
- 不另建知识包发布体系、知识任务身份或报告节点类型。
- 不把所有 PACK、未采用的引用、完整对话或模型内部推理过程存入快照。
- 不承诺备份全部用户资料或外部来源；知识快照保留方法依据，不等于外部事实材料的备份。
- 不建立解释依赖图、自动有效性判断或自动重跑机制。

## 验收与验证证据

| 场景 | 可观察结果 |
|---|---|
| 纯知识 PACK | 无 Python 入口也能被发现；名称和描述支持选择，所选正文和所需引用可读取。 |
| 知识领域 | 纯知识路径可使用通用代码评审或团队交付规范，平台不因其不属于性能分析而拒绝；用途以 PACK 声明为准。 |
| 无匹配入口 | 明确报告知识入口与 Workflow 的能力缺口，不自动执行任务或创建 PACK；经验整理与 PACK 创作须获得用户授权。 |
| 纯知识任务 | 形成并成功保存目标与结果；Session 的 Run inventory 与报告节点集合为空。 |
| 知识驱动取证 | 实际选择、调用已有 Workflow；Run 身份、证据、Guide 与解释符合既有合同。 |
| 按需加载 | 未选中入口及未采用引用不被预先加载、快照；共享经验无需维护两份正文。 |
| 引用范围 | 相邻材料及同 PACK 的共享文档可按相对链接读取；嵌套引用按来源文档定位；最终目标位于 PACK 外的知识引用被拒绝。 |
| 文件范围 | Markdown、文本模板和代码示例按 UTF-8 原文读取和恢复；非 UTF-8 与不支持的二进制知识引用明确失败，示例代码不因读取而执行。 |
| 程序捕获知识 | Session 选用读取的成功正文与已保存快照来自同一次原文读取；原文件随后变化不改变该快照。能力浏览不产生任务记录。 |
| 操作分离 | `inspect knowledge` 只读浏览当前知识；任务读取首次保存快照，再次读取默认沿用已有快照。 |
| 跨任务恢复 | 新任务仅凭 Session ID 找回目标、结果与原方法依据，不靠旧对话补造。 |
| PACK 升级或移除 | 两条路径均可恢复已保存知识；新版不自动替代原依据，已保存的结果不因 PACK 缺失而消失。 |
| 明确更新方法 | 新旧知识以不同材料 ID 保存，旧快照不可覆盖；正文指出实际采用的材料，旧依据仍可读取，新版不冒充旧任务原依据。 |
| 多份快照选择 | 唯一正文快照可按入口名读取；同一入口有多份时按材料 ID 读取，未指定则报告歧义，不默选最新快照；不同引用文件不产生入口正文版本歧义。 |
| 旧记录兼容 | 新版完整读取 schema 2 的目标、查询材料和报告；只读不改写，成功写入才保存为 schema 3；失败保留原文件，未知版本明确拒绝。 |
| 并发与修订 | 并发首次采用相同目标仅追加一份原文；明确更新遇到修订冲突不追加；回执使 AI 能识别并核对未读并发变化。 |
| 未保存的引用 | 恢复旧入口后遇到未保存且已不可取得的引用时说明缺口，不将当前文件当作旧依据，也不宣称已经恢复未保存内容。 |
| 失败与授权 | 读取或保存失败不宣称快照、结果已可恢复；损坏不冒充无记录；工具仍遵守任务授权。 |
| 回归 | 程序 Workflow、Run Guide、真实调用关系、Query 和现有分析保存恢复保持原合同。 |

程序行为使用公开 CLI 集成测试验证；AI 的入口选择、知识应用、按需取证和跨任务恢复使用隔离安装产物上的真实任务验证。结果记录实际命令、保存内容和任务交付，不以 AI 复述流程作为通过依据。

### 实施与当前验证

实现位于 CLI 的 knowledge.rs、knowledge_read.rs、inspect.rs、lib.rs 与现有 analysis_record/store；YAML 声明复用 [serde_yaml_ng](https://docs.rs/serde_yaml_ng/latest/serde_yaml_ng/)，未自建通用解析器。Entry 与 Reference 的加载复用原 PACK discovery，追加材料复用原记录锁与完整替换机制，不引入独立状态库。

验证环境：Windows x86_64 客户端；实施目录为隔离 managed worktree `issue-312-knowledge-entries/kat-cli`，起点为上述 291e297。以下命令均在该 worktree 根执行；临时 Runtime、安装集合、输入与 Session 仅用于验收，不进入交付内容。

- `cargo test -p kat-cli --test knowledge`：12 通过、1 个真实 Host 测试默认忽略；覆盖纯知识、恢复、更新、多版本歧义、并发首次去重、路径/声明/格式失败、schema 2 兼容、Windows 替换失败保留、junction 最终目标边界和无关损坏 Run 不阻断知识读取。
- `cargo test -p kat-cli` 全套默认测试通过；保留已有干净 Windows profile 和真实 Host 测试的默认忽略。既有共享 test support 在 analysis_archives 编译中有两项 dead_code 警告，未扩大本次范围修改。
- 使用锁定且校验 SHA-256 的 CPython 3.14.6、uv 0.11.28，从本 checkout 构建 Workflow、SDK 与 Datasource wheel；依赖按 Windows lock 的哈希下载。设置 KAT_TEST_PYTHON、KAT_TEST_WORKFLOW_WHEEL、KAT_TEST_SDK_WHEEL、KAT_TEST_DATASOURCE_WHEEL 后，`cargo test -p kat-cli --test knowledge real_host_combines_knowledge_and_workflow_and_recovers_without_pack -- --ignored --exact` 及 `cargo test -p kat-cli --test analysis_real_host -- --ignored` 均通过。后者覆盖既有嵌套 Run、各自 Guide、Query 材料与移除 PACK 后恢复。
- `cargo check -p kat-cli` 通过；修改 Rust 文件的 rustfmt 与 git diff --check 通过。以上是补充诊断，不代替下述必需 MCP 验证。
- 四个受影响产品 Skill 和新示例入口的 quick_validate 通过；`python -m unittest discover -s build/tests -p test_skill_collection.py`、`test_assemble_skill.py`、`test_scaffold_pack.py` 共 27 项，22 通过、5 因缺平台验收输入跳过。集合测试验证搬移后链接与部署路径，不冒称已运行 Linux Payload。
- 独立 AI 使用搬移的五 Skill 集合与当前 CLI、真实方法文件完成纯知识事实评审，保存两份知识材料、revision 4、零节点；没有 Python Host。Session `01a1292a-b397-7f90-a5f6-0a75a2c08650` 的方法 ID 为 `01a1292b-00c4-7cc0-b5b1-f302864841d1` 与 `01a1292b-1ecb-7d30-abbb-4ca061ac57fe`。移走 PACK 后，另一新 AI 任务仅凭 Session ID 恢复目标、报告与两份原方法，revision 保持 4，不重新执行或补造测量事实。
- 混合独立 AI 任务使用搬移集合及当前三个 wheel，按入口选择实际 sample-check/sample-summary，保存两份知识与两份真实 Query 材料；Session `01a12933-464a-7513-b2bc-300c8146ee5d`，真实 Run `01a12933-c81d-7f12-95cd-2a15c5bc428f`，report revision 6。查询给定两行 CSV 得到降幅 -5% 至 12.5%、算术平均 3.75%，指出全体下限承诺没有支持；报告只含这个真实 Run。移走 PACK 和来源后，另一新 AI 任务仅凭 Session ID 通过六次公开只读调用恢复报告、两份原方法、两份真实查询原文及执行时 Guide，revision 保持 6，不重新执行或查询，也不声称原输入全部可重现。方法材料 ID 为 `01a12933-880b-7460-bdac-80004483b35a`、`01a12933-aad2-79a1-998a-efee9fb18cb3`；事实材料 ID 为 `01a12933-f095-7572-8a21-6dd87aa3f666`、`01a12934-2182-7682-8261-756a3d5fe8e4`。
- 双轴静态审查覆盖实现与后续测试/合同增量：Standards 0 项可行动发现；Spec 0 项可确认缺陷，未把待验收当作已交付。两次审查均保留上述 MCP 门禁为未验证限制；最终恢复段仅补充实际已完成证据，不改变生产实现。
- rust-coding 要求的 rust_diagnostics 工具当前未提供，VERIFY 为 blocked。应调用 `{mode:"full",workingDirectory:<本 worktree 根>}` 和 `{mode:"test",workingDirectory:<本 worktree 根>}`，默认全 targets/features；不能以手写 cargo 输出宣称替代。全量 workspace MCP 验证未执行；Linux 真实运行与干净 Windows profile 门禁不属于已通过证据。

### 2026-10-11：kat-analyze 共同任务主线增量

本次从 `3888b56ba788477d22311d0ccd4d95ab0e1f203f` 起只调整 Skill 文档和本规格：共同流程负责发现、选择、Session、保存与恢复，知识快照和 Workflow 取证作为按需操作引用；不改 CLI、SDK、存储或 Run 合同。消除先猜任务类型的入口分叉，并保留已有适用 Workflow 优先、无 Host 的文字评审及发现失败不等于空列表的边界。

在搬移后的五 Skill 安装副本、Windows x64 客户端和每例隔离 Data Home 上，独立新 AI 仅收到原始问题与授权材料。候选含无关标题改写知识，未给出内部类型、入口名、答案或调用序列。验收仅经公开 CLI，未让执行者读取仓库、PACK 源码或私有 Session 文件。实际保存结果另经 `analysis show` 核对：

| 用户问题与实际选择 | 真实 Session | 保存结果 |
|---|---|---|
| 只评审一段文字；发现候选后采用证据准则，不索要 CSV 或执行样本计算 | `01a12a03-19f1-76d2-9978-0386b13aa762` | revision 4；2 份知识、零 Run/Query/节点 |
| CSV 计算排序与平均值；同 PACK 两类能力并存，但不采用收益评审或标题知识 | `01a12a03-8dfe-7c12-9efb-5ddca3707811` | revision 3；Run `01a12a04-2b97-7941-85a6-f6a1d176664b`，1 份 Query；结果 -10%、25%，算术平均 7.5% |
| CSV 核验全体最低收益声明；采用范围准则并执行现有程序 | `01a12a03-d7bd-77b3-9a5b-3676e8cc906d` | revision 6；2 份知识、2 份 Query、Run `01a12a04-9dfb-7ae2-a44f-96fcb056104a`；反例 -5%，平均 3.75% |
| 没有 Python，仅评审“5 人平均收益即每名用户最低收益”；披露 Workflow 发现受限后采用足够的文字准则 | `01a12a04-2123-7cf3-8127-817a124dbae5` | revision 4；2 份知识、零 Run/Query/节点；不宣称不存在 Workflow |

缺 CSV、Workflow 导入失败、没有照片转 3D 能力三例分别通过 `inspect`、相关知识/Workflow 列表和必要参数详情确认缺项、失败或不匹配；均无 Session、Run、采纳或安装写入。缺输入只请求授权 CSV，导入失败没有用通用工具重算，无匹配没有采纳无关标题能力或自动创作 PACK。原始计算夹具两次因遗漏普通目录和 Guide 而失败，保留受阻报告但不计为成功验收；修正夹具后另起干净任务验证，不改产品来掩盖夹具错误。

纯 Workflow 的干净重验 Session `01a12a09-fa6a-70a0-b86e-bd3be95581b9`、Run `01a12a0a-567a-76e2-9f36-21d03eff8bbe` 得到 -10%、5%、20%，算术平均 5%；另一输入复验 Session `01a12a0a-0057-7270-b9bb-cd1ccbbc92a0`、Run `01a12a0a-4f03-7800-bf62-e585d9fa2f4d` 得到 -25%、20%，平均 -2.5%。两者均先发现两类能力、查看参数，保存目标后执行并归档排序/平均值查询，再保存 Run 解释与总报告；revision 3，各 1 份 Query，无知识采纳。混合复验 Session `01a12a0a-3c74-73e1-b7f2-72e182ca37b4`、Run `01a12a0a-c22e-74a1-8e72-67880c78fb9c` 采用两份方法、归档两份查询后保存 revision 6；三条样本只有两条达到 15%，0% 反例不支持全体承诺。上述保存内容均另经公开 `analysis show` 核对。

移走本轮纯知识 PACK 后，新 AI 仅凭 Session `01a12a04-2123-7cf3-8127-817a124dbae5`，用 5 次公开只读调用恢复报告、两份原方法并核对 inventory，revision 始终为 4，无 Host、无 Run。移走本轮混合 PACK 和原 CSV 后，新 AI 仅凭 Session `01a12a03-d7bd-77b3-9a5b-3676e8cc906d`，用 8 次公开只读调用恢复报告、四份材料、inventory 和执行时 Guide，revision 始终为 6。两例都以 `analysis show --session S` 开始，按实际材料 ID 读取，未重新全局发现、查询、执行或保存；没有用新文件补造历史方法，也未声称原 CSV 已备份。共 12 例有效行为演练（7 次正向、3 次缺项/失败/无匹配、2 次恢复）符合本轮检查点；两次无效夹具失败不计入，少量演练不构成模型始终正确的保证。

`quick_validate`、`git diff --check` 和集合安装/搬移测试 10 项通过；全量 `build/tests` 共 90 项，85 通过、5 因既有平台验收输入缺失跳过。全量构建单测最初因系统 Python 缺少 SDK 声明的构建依赖无法加载；在临时验证 venv 安装锁定的 setuptools 80.9.0、griffelib 2.3.0、markdown-it-py 4.2.0 后通过，未改系统或发布载荷依赖。`cargo test -p kat-cli` 默认全套通过，已有忽略项和警告保留。格式/搬移测试不代替 AI 行为验证，本轮未运行 Linux Payload；前述必需 MCP、干净客户端和全量 workspace 门禁限制不变。

两路独立静态审查以用户确认的 `291e29768f844a27b52b2d450f53d2f203ab2c27` 比较到 `0e09b52b1aa5b13d23b902c270e466fdb007d968` 的完整 PR，并补审上述重复验收与恢复证据增量：Standards 0 项可行动发现，Spec 0 项可确认缺陷。未把静态审查当作执行验证，仍保持 Draft 和原有待验收门禁。

### 门禁补齐（2026-10-11）

本轮只补齐验收证据和现有 CI 的执行覆盖，不修改知识、Workflow 或 Session 的生产合同，不降低门禁阈值，不提升 Windows 正式支持级别。此前段落的工具缺失、未运行 Linux 与 Draft 描述是当时状态，以本节的实际结果为准。

- PR Guard 的 issue 关联改为 `Refs #312`；用户明确批准当前端到端切片规模后，添加 `approved-large-change`。门禁运行 [38125326667](https://github.com/maokelong/kat-cli/actions/runs/38125326667) 通过；该标签只豁免规模，不豁免执行验收。Guard 自身单测 19 项通过。
- 在本 managed worktree 根，通过标准 MCP SDK 的 stdio 客户端连接已安装 RustBot 的原始 `rust-diagnostics/server.ts`，实际执行 `rust_diagnostics` 的 `full`，再执行 `test`。服务与锁定依赖只复制到隔离临时目录，未修改服务实现或全局 Codex 配置。原文件与副本 SHA-256 同为 `3b50faf73f0894773c8307fe3e8878046aca353dd9c1c10622c6dd3df0677e56`。
- 检查代码版本为 `1f5f8ef753fa8ebb624185732c31bf3267031df6`，Rust/Cargo 1.95.0；两次调用均指定 `workspace=true`、`locked=true`，保留默认全 targets/features。`full` 实际运行 fmt、check、clippy，三条命令退出码均为 0；原始 metadata 状态为 `passed`，MCP 响应为 `ok` / `statusDetail=passed`。两项既有 test support dead_code 在 check/clippy 中各出现一次，无错误或新增告警。
- `test` 实际运行 `cargo test --all-targets --all-features --workspace --locked`，退出码 0，240 通过、13 忽略、0 失败；原始 metadata 为 `passed`。默认忽略不构成真实 Host 验收，因此将现有 `real_host_combines_knowledge_and_workflow_and_recovers_without_pack` 接入 Full CI 的双平台真实 Host 步骤，与已有 wheel 构建及安装共用输入，不增加新 job 或占位测试。
- 双平台 Full CI 与 Build Payloads 在最终提交上复验；完成后补记实际运行链接与结果。按照 ADR-0002 和 `release-rehearsal.md`，PR 集成门禁与发行级验收分开记录。干净 Windows 10/11 客户端的正式支持验收仍归 #143；GitHub Windows builder 或本开发机测试不冒称该验收通过，本 PR 保留 Windows pre-release candidate 状态。

## 公开接口方案

### 入口身份与声明

入口公共身份为 `(pack, entry)`；不同 PACK 可有同名入口。`entry` 使用与 PACK name 相同的可移植小写 ASCII kebab-case 规则及 Windows 设备名限制，并与 `skills/<entry>/` 目录名一致。Workflow 与知识入口属于不同选择器，可以同名，不互相覆盖。

`SKILL.md` 以 YAML frontmatter 声明必填 string `name` 和 `description`，首版只接受这两个字段，未知字段、重复字段、非法名称、空描述或 frontmatter 后空正文均失败。`description` 按既有展示字段约定 trim，原文件不改写；正文与快照保留完整原文，包含 frontmatter、换行和空白。PACK 的领域与用途使用既有 `pack.toml.description` 及入口描述表达，不增加 `domain` manifest 字段。

只发现 `skills/` 一级入口目录中的 `SKILL.md`；没有 `skills/` 或没有入口是完整空列表。列表校验所有声明后按名称排序，只返回元数据，不解析正文语义、不读取引用、不导入或执行 PACK Python。发现已安装 SDK 的根沿用既有定位机制；没有 Python Host 时静态 PACK 仍可发现。缺失引用不影响列表，实际请求该引用时再失败。选定单个入口只校验该入口，不加载其他入口、Workflow 或 Provider。

YAML 由成熟解析库处理，不自建通用 YAML 解析器；frontmatter 分隔与位置判断仅保留必要胶水。Markdown 是不透明原文，KAT 不执行其中指令、不渲染模板、不解析正文建立知识依赖图；这些内容由 AI 在任务授权范围内应用。

### 选择器

下列均是设计命令，`P` 为 PACK name，`E` 为入口名，`F` 为 PACK 根相对文件路径，`S` 为 Session ID，`M` 为材料 ID，`R` 为已核对的修订号。

| 命令 | 语义 |
|---|---|
| `kat inspect knowledge --pack P` | 当前 PACK 的入口元数据列表。 |
| `kat inspect knowledge --pack P --entry E` | 浏览当前入口完整原文。 |
| `kat inspect knowledge --pack P --path F` | 浏览当前 PACK 的一个 UTF-8 引用文件。 |
| `kat knowledge read --session S --pack P --entry E` | 首次采用并快照入口，已有唯一快照则重读原文。 |
| `kat knowledge read --session S --pack P --path F` | 首次采用并快照引用，已有唯一快照则重读原文。 |
| `kat knowledge read --session S --material M` | 只读恢复一个知识快照，不访问当前 PACK。 |
| `kat knowledge read --session S --pack P --entry E --update --expected-revision R` | 明确采用当前入口的新快照，保留旧材料。 |
| `kat knowledge read --session S --pack P --path F --update --expected-revision R` | 明确采用当前引用的新快照，保留旧材料。 |
| `kat analysis show --session S --material M` | 沿用既有材料读取入口，同样可恢复知识快照。 |

`--entry` 与 `--path` 互斥；任务读取必须二选一或指定 `--material`，不能无目标读取全部知识。`--material` 与 PACK、入口、路径、更新、修订号及 `--pack-dir` 互斥；若 ID 对应查询材料，`knowledge read` 明确报类型不匹配，查询材料仍由 `analysis show` 读取。`--expected-revision` 只与 `--update` 成对出现，普通首次采用沿用现有材料追加的锁与回执规则。

需要当前 PACK 时，支持既有可重复 `--pack-dir` 精确 candidate，不增加根目录或优先级机制。按名称或路径恢复时先读取 Session 快照；已命中则无需发现 PACK，也不因当前 PACK 的缺失、重名或 Python 不可用而失败。明确 `--update` 总是需要解析当前 PACK，不能悄悄降级为原快照。

`F` 是以 `/` 分隔的 PACK 根相对路径，不是 OS 绝对路径或 URL。AI 按返回文件位置解析 Markdown 相对链接、规范化为 `F` 后明确请求；段落锚点不进入文件选择器。KAT 使用标准路径能力校验普通文件与实际解析后的 PACK 归属，拒绝绝对路径、盘符、路径逃逸和不支持的文件格式，不自动跟随链接。返回的路径不包含物理部署根。入口 `skills/<E>/SKILL.md` 必须通过 `--entry E` 读取，不能用 `--path` 绕过声明校验。

引用材料按 `(pack, path)` 查找已存快照，入口按 `(pack, entry)` 查找正文快照；多个匹配均报告歧义，并提供候选材料 ID 供明确选择。引用文件不继承入口正文的版本身份。首次读取尚未保存的引用捕获的是读取时当前内容，不声称它是入口首次采用时的历史版本；恢复任务需要补用当前引用时，先说明这一缺口，再依已确认的更新方法边界处理。

### 成功响应与材料

继续使用既有 KAT Response，以下字段均位于 `result`。列表返回 `entries` 数组，每项只有 `name`、`description`；入口浏览返回 `entry` 对象，包含 `name`、`description`、`path`、`content`；引用浏览返回 `reference` 对象，包含 `path`、`content`。空列表不是失败，其他选择未命中不能伪装成空正文。

在 Analysis Record 的 `materials` 增加 `kind="knowledge"`，完整材料如下，不另建快照存储或身份体系：

```json
{
  "kind": "knowledge",
  "pack": "review-pack",
  "path": "skills/code-review/SKILL.md",
  "entry": {"name": "code-review", "description": "按团队准则评审代码"},
  "content": "完整 UTF-8 原文，包含入口 frontmatter"
}
```

引用材料的 `entry` 固定为 `null`。概要展示 `kind`、`pack`、`path` 和入口名或 `null`，不把正文塞入整个报告概览。`analysis show --material M` 沿用 `session_id`、`revision`、`material_id`、`material`；`knowledge read` 在此基础上包含 `previous_revision`。新追加时修订号增加一次；重读已有快照时两修订号相同，不产生新材料。材料 ID 使用现有生成机制，原文不可由 `analysis save` 覆盖。

### 保存、并发与格式兼容

首次任务采用要求 Session 已存在且其目标已通过首次 `analysis save` 保存；未保存目标时明确提示先保存，不自动虚构目标或创建 Session。首次采用不是先给 AI 内容再补归档：程序从来源读取一次，持有 Session 租约，在稳定记录锁内核对已存材料并提交，再返回同一份持久原文。并发首次采用同一入口或文件只保留一份快照；后到请求返回已经提交的材料，不将其竞争期间取得的内容冒充已保存原文；若锁内核对发现已有多份，则报告歧义，不能再追加或任意代选。

`--update` 在同一稳定锁内比较 `--expected-revision`，只有匹配才追加新快照。普通归档的 `previous_revision` 与 `revision` 复用既有规则：回执显示出现未读并发修改时，AI 先 `analysis show` 核对，不用最新数字绕过未读内容。明确更新的响应不确定时先恢复核对，不盲目重复 `--update`。

继续复用现有完整记录替换和失败保留机制；读取保持不可变材料语义，不扫描或验证无关 Run 才返回知识原文。记录不存在、损坏、未知格式、引用不可取得、声明非法与多快照歧义分别给出可处理诊断；失败不返回部分成功。

Analysis Record 新写入使用 schema 3；新版仍读取 schema 2 的既有查询材料与报告，读取不改写磁盘，首次成功写入时再按相同记录合同保存为 schema 3。schema 2 不能含知识材料，未知版本仍失败；不增加通用迁移框架，也不承诺旧版 CLI 读取新格式。旧记录回归与失败不破坏旧文件属于正式验收，不把分析记录当作可丢弃缓存。

### 用户与作者路径

`kat-analyze` 使用一条共同任务主线：明确目标或恢复任务，发现候选并理解适用条件，组织方法与程序取得事实，按判断继续取证或保存交付。先从 PACK 描述选择范围，再查看相关知识入口与 Workflow 的用途摘要，按需读取候选详情后选择，不先给问题分类，不要求用户决定使用哪一类能力，也不强制知识先行或两类都采用。已有适用 Workflow 提供的程序能力优先复用，不能通过不选定它而用通用工具重做。

Session 初始化、保存、修订核对与恢复集中在共同流程；知识引用与快照、Workflow 执行与 Run 解释各自按需读取操作 reference。无 Run、无 Query 和无节点不是例外任务类型，而是未采用这些操作的正常状态。不改变 CLI、记录格式、授权或真实 Run 关系。

能力发现区分成功空列表、候选不适用、缺少输入和 inspection 失败；不要求扫描全部 PACK 或加载全部正文。Workflow inspection 需要 Python，缺少 Host 时不能把发现失败当作没有 Workflow；说明发现受限，仅在已发现知识明确足够且任务不依赖程序能力时允许继续，保持无 Python 的纯知识路径。所需能力无法确认时报告阻碍，不静默换路或安装依赖。只读能力浏览或恢复不创建记录；已有 Session 优先恢复原方法和事实，不重新全局发现。

本轮验证使用搬移安装副本与隔离 Data Home，仅给独立 AI 用户问题和授权材料，不提供类型标签、入口名、预期答案或调用顺序；候选包含无关能力。覆盖仅知识、仅 Workflow、混合取证、两类存在但只需一种、优先复用程序、无匹配/缺输入/发现失败、无 Python 与仅 Session ID 恢复；关键选择再用另一份问题复验。格式和链接测试不替代行为验证，记录实际公开调用、身份、保存回执与结论，验收夹具和中间产物不进入交付。

恢复从 `analysis show` 的目标、正文、知识材料概要开始，只按需读取被引用的材料 ID。更新方法由 AI 说明采用的新材料及对旧结论的影响，不引入程序依赖图或自动重执行。

`kat-author` 补充纯知识 PACK 的创建、入口格式与引用示例，无 Python 时不要求创建空 Workflow 或运行程序测试。原有 `kat test` 保持程序测试合同；知识声明和读取通过公开 CLI 验证，不新增另一套知识执行宿主或模型评分命令。知识入口随现有 PACK 发布和安装，验收夹具与作者示例不自动成为默认生产能力。

### 实施顺序与验证落点

一个完整切片内按可 review 的顺序实施，不单独发布半可用入口：

1. Rust CLI 增加静态知识声明读取与只读 inspection，复用 PACK 发现和 Response；公开 CLI 测试覆盖列表、格式、相对引用、UTF-8、边界与无关 Python 不加载。
2. 扩展 `analysis_record`、`analysis_store` 和材料读取，增加任务采用与明确更新命令；测试捕获原文一致性、竞争、修订冲突、保存失败、PACK 移除、旧格式与多快照。
3. 同步现有五 Skill 集合中实际相关的四个产品入口、公共命令合同、作者示例和发布装配校验；保持共享载荷和现有 Workflow/Run/Query 合同。
4. 从隔离安装并搬移后的产物执行纯知识与混合取证两条验收，再用新的 AI 任务仅凭 Session ID 恢复；记录实际结果与限制，不能以 CLI fixture 冒充 AI 路由验证。

预计改动集中在 `kat/platform/cli/src/{inspect,main,analysis,analysis_record,analysis_store}.rs`、必要的新 `knowledge` 模块、CLI 集成测试及对应产品 Skill 文档；源码路径与改动范围以实施时主线为准。不修改原生 Datasource 或无关 PACK 的算法，不直接在当前落后且有既存改动的任务分支叠加生产实现。

## 对现有合同的影响

ADR-0078 的 Workflow 确定性执行边界保留，知识只能依附程序入口的限制由 ADR-0088 扩展。Workflow Guide 和 Run Guide 快照保持所属 Run 的解释职责，新增知识入口及 Session 知识快照承载任务方法。

ADR-0017 的固定 PACK 布局增加 `skills/` 知识入口树，其既有 Python 入口及导入边界不变。

`kat-analyze` 的能力选择扩展为可选知识入口或 Workflow；匹配知识入口后，直接处理受授权任务材料或按需执行 Workflow。现有 Workflow 参数准备、程序执行、证据查询和 Run 解释流程保持各自合同，不把纯知识路径伪装成无输出 Workflow。

作者侧补充知识入口声明、引用复用和验收说明；生产实现、Skill 主文件和普通用户文档必须在实际交付时同步，不因本设计记录而提前宣称功能可用。
