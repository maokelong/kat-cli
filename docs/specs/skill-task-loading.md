# KAT Skill 按任务组织执行知识

关联 [Issue #322](https://github.com/maokelong/kat-cli/issues/322)。基线为 `beb81cce` 的五个同级 Skill。

## 问题与最小切片

任务入口默认读取覆盖所有命令的共享合同，常规分析又需要跨四份流程文档拼接指令。作者与 SDK 入口混合不同目标，部分资源位置、PACK 发现范围与 Guide 取得方式说明不完整或相互矛盾。

本切片调整五个入口的内容组织、命令用法与资源引用，减少常规任务的额外读取。保持 CLI、Runtime、公共 API、载荷布局、Data Home、环境检查频率和 SDK 文档人工交付方式。不增加启动器、缓存、流程校验器或门禁。

## 内容归属

允许在任务流程附近重复少量常用命令用法。共享字段、平台定位与失败语义保留唯一来源；操作专用返回事实由对应任务说明，其他使用点不复制完整字段合同。

`kat/references/command-reference.md` 保持原路径，收敛为共享执行合同：部署、Data Home、PACK 范围、跨任务返回事实、查询结果分支与共用失败边界。命令用法直接进入对应任务入口，任务 Skill 可以直接调用，无需经过总路由。

合并后的入口醒目强调“必须遵守以下流程”，用步骤与目标分支突出原有要求。内容增加不代表所有分支都必须执行；流程强调只通过文案完成，不新增程序校验或逐步确认。

## 任务文档迁移

| 任务 | 常规路径 | 条件资料 |
| --- | --- | --- |
| 分析 | `kat-analyze/SKILL.md` 包含目标恢复、能力选择、Session 初始化、知识/Workflow 使用、保存与交付 | `analysis-recovery.md` 承接冲突、响应不确定、多快照、显式换方法和删除 |
| 作者 | `kat-author/SKILL.md` 按只读、骨架、知识、程序、函数库组织，合入复用检查及交付要求 | 框架 API、SDK 短目录与候选 reference；复杂执行读 `python-contracts.md`，函数库读 `pack-helpers.md` |
| SDK | `kat-dev-sdk/SKILL.md` 按能力开发、API 文档、wheel 构建选择流程 | `api-documentation.md` 只用于文档生成，`sdk-build.md` 用于构建与验收 |
| 复核 | `kat-review/SKILL.md` 保持单文件，只读现存记录与证据 | 使用短共享合同，不加载分析写入流程 |

删除迁移完成的分析四份 reference、作者 `reuse-check.md`、`knowledge-authoring.md`、`result-contract.md`。作者长篇创作流程提取常规内容后整理为 `python-contracts.md`；SDK `sdk-development.md` 的能力开发内容并入入口，文档生成内容迁至 `api-documentation.md`。更新调用方及 UI 描述，不保留并行旧流程。

正常分析只需任务入口和共享合同，特殊情况出现后再读恢复资料。用户已给精确目标时直接验证 detail，未知目标才渐进发现；已有且仍适用的 detail 复用，执行前不重复读取。纯知识任务不制造 Run，已有适用报告可直接只读交付。

纯知识创作和空骨架不要求 SDK API、完整 Python 合同或 pytest。程序修改保留必要的复用、inspection、测试与 Guide 关联检查。只构建 SDK wheel 不等待无关 API 文档生成、审核或作者侧复制。

## 部署与 PACK 定位

从实际加载的 SKILL.md 绝对位置解析任务根及相邻共享 `kat/`，再按现有平台规则取得唯一载荷。补齐 PowerShell 调用及空格路径示例；资源缺失报告已检查的位置，不扩大搜索或拼接其他版本。作者 API、脚本和示例明确链接；源码示例只从已明确的仓库根读取，不将未随包材料视为安装缺件。

PACK 范围为共享根的 `assets/packs/`、当前 Data Home 的 `packs/`、CLI 自动加入的安装 SDK，以及精确 `--pack-dir`。两个默认根只扫直接子目录；显式目录直接包含 manifest。同目录去重，不同目录同名失败，无覆盖顺序。

外部 PACK 的相关 discovery、采纳与 run 持续携带目录。已有 Run、记录和材料 ID 的读取不重新发现 PACK。区分空列表、未进入范围、不适用与失败；缺位置时请求最小信息，不全局扫描。

## 方法与文档边界

Workflow detail 提供用途与参数，Guide 由执行捕获并通过成功 Run 或 inspect run 获取。作者检查获授权维护的文件与装饰器关联，不为读取 Guide 自动执行；Provider Guide 仍从 detail 取得。

作者侧 SDK API 资料继续由用户提供与 wheel 配对的审核版本。明确缺项和源码侧位置，SDK 更新后说明人工资料维护责任，不自动复制、不比较运行时版本。知识任务与其他不依赖缺失资料的工作可以继续。

保持成功 Response 才能使用结果、revision 冲突不得盲目重提、知识更新需用户明确要求、只读复核不写原记录等既有边界。

## 验证

复用现有 Skill 集合、装配与重定位测试，并运行 skill-creator 的 frontmatter 检查。按代表场景走查知识分析、Workflow 分析、已有目标、Session 恢复、复核、知识/骨架创作、程序开发、SDK 独立构建与外部 PACK 路径。

比较旧版和新版的常规必读文件数量与字节规模，只作为结构成本证据；不将其表述为实测分析耗时。未提供实际安装包位置和真实任务输入时，不声称已复现或修复特定安装环境，也不进行生产 Workflow 执行。

### 本次验证结果

2026-10-16，在 Windows 主机完成：

- `python -I -B -m unittest discover -s build/tests -p test_skill_collection.py -v`：10 项通过，含归档重定位、同级共享路径及本地引用检查。
- `python -I -B -m unittest discover -s build/tests -p test_assemble_skill.py -v`：9 项通过，5 项因当前 Windows 无创建符号链接权限而跳过。装配、真实手册复制与集合移动通过；不能据此声称本机验证了符号链接场景。
- 用 `python -X utf8 -I -B` 运行 skill-creator 的 `quick_validate.py`：五个 Skill 均通过。显式 UTF-8 用于避免验证器按 Windows GBK 解码中文文件。
- `git diff --check` 通过。复杂 Context、数据、物化和 Provider 原合同段落与迁移前内容核对一致。

常规指令材料的结构成本如下。字节数统一按 UTF-8/LF 计算，含任务入口与共享合同，不含用户输入、PACK 方法与按需 API 文档；这是按规定读取路径计算的结果，并非真实 AI 耗时测试。

| 场景 | 必读文件 | 字节数 | 减少 |
| --- | --- | --- | --- |
| Workflow 分析 | 5 → 2 | 30,771 → 18,771 | 39.0% |
| 纯知识分析 | 5 → 2 | 29,313 → 18,771 | 36.0% |
| 混合分析 | 6 → 2 | 32,967 → 18,771 | 43.1% |
| 纯知识创作 | 5 → 2 | 52,937 → 18,218 | 65.6% |
| 复核 | 2 → 2 | 23,486 → 13,933 | 40.7% |

共享合同由 16,746 降至 7,370 字节。静态场景走查确认：已知能力先验证 detail；只读恢复与复核不写原记录；知识任务与骨架不读取完整程序资料；独立 SDK 构建不等待文档交付；外部 PACK 相关调用携带目录；特殊恢复沿用原 revision 与快照规则。

以上未执行生产分析、真实 Trace 或安装到用户现用 Skill 目录，实际运行耗时和特定安装环境问题仍需相应输入验证。
