---
name: kat-analyze
description: 使用 KAT 发现并执行已有 Workflow，查询证据、保存分析报告，或仅凭 Session ID 恢复报告树与原依据并继续分析。适用于新分析、恢复已有分析或临时组合多个已有 Workflow 取证。
---

# KAT Analyze

围绕一个用户问题执行已有分析能力，将所选 Run、解释和原依据保存到该 Session 的 Analysis Record，形成一份可恢复的综合报告。

`kat analysis` 由本 Skill 在已授权的分析过程中自动调用。初始化、保存进展和报告是分析职责，不以用户提出“保存”或知道命令为前提，也不逐步询问是否记录。用户只需描述问题，继续已有分析时提供 Session ID；JSON 请求、材料引用和 revision 由 AI 管理，不交给用户手动填写。

仅浏览能力时不创建分析记录；只读恢复已有有效报告时不初始化或重写。用户明确限制为只读时遵守该范围，独立复核按 `kat-review` 的只读边界执行。

## 定位共享部署

从本 `SKILL.md` 的绝对位置解析相邻 `../kat/`，确认其中的 `SKILL.md`，读取 [公共命令合同](../kat/references/command-reference.md)。该 `kat/SKILL.md` 的父目录才是公共合同中的 `<kat-root>`；不要把当前任务目录或工作目录当作载荷根。

每次调用按公共合同选择 CLI 的绝对路径，直接沿用当前 Data Home，不主动询问是否修改目录；按 Response 与失败边界交付。四个 Skill 必须来自同一套部署；缺少共享目录或载荷时说明部署缺项，不从其他版本或 `PATH` 拼装。直接使用本 Skill 无需先调用总路由。

## 分析与交付

1. 开始分析前读取 [分析流程](references/analysis-flow.md)，在各触发时机完成其中的必做步骤。已有 Session 先恢复记录，复用仍适用的结论；新分析选定首个 Workflow 后，在读取 detail 和执行前初始化目标。
2. 只从 Workflow 知识发现分析能力，不扫描 Provider，不把输入路径或日志当作成功结果。
3. 没有匹配 Workflow 时说明已发现的能力边界；未经用户明确授权，不切换到 PACK 创作或修改源码。
4. 成功选用 Run 后立即记录节点与已知选择依据；实际采用的 Guide 和关键查询证据先归档，已形成解释先保存再供后续分析复用。交付前读取 [分析结果契约](references/result-contract.md)，保存总报告并确认成功。正常交付只说明结论、证据、限制、保存状态和 Session ID，不要求用户理解后台命令。证据足够即结束，不自动触发 `kat-review`。
