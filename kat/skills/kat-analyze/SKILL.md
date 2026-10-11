---
name: kat-analyze
description: 围绕用户问题发现并组织 KAT 已有能力，应用经验、按需执行 Workflow 取证，保存依据与报告；也可凭 Session ID 恢复并继续分析。
---

# KAT Analyze

围绕一个用户目标组织已有能力，形成有事实依据、可恢复的分析结果。知识入口指导任务，Workflow 承担确定性执行，其 Guide 解释所属 Run；它们可以独立使用或组合，不是需要用户预先选择的任务类型。Session 统一归属方法、真实执行与分析正文。

## 定位共享部署

从本 `SKILL.md` 的绝对位置解析相邻 `../kat/`，确认其中的 `SKILL.md`，读取[公共命令合同](../kat/references/command-reference.md)。该 `kat/SKILL.md` 的父目录才是 `<kat-root>`，不使用当前任务目录。每次按合同选择 CLI 绝对路径，沿用当前 Data Home；各 Skill 必须来自同一套部署，缺项时不从其他版本或 `PATH` 拼装。直接使用本 Skill 无需先调用总路由。

## 问题分析流程（必须遵守）

开始时读取[共同分析流程](references/analysis-flow.md)，按这一条任务主线推进：

1. 明确目标或恢复已有任务。
2. 发现相关 PACK 中的知识入口与 Workflow，按用途和适用条件选择，不先猜任务属于哪一类。
3. 应用所选方法、按需取得事实，判断是否足以回答问题；不足时继续发现与取证。
4. 保存必要进展和总报告，按[结果契约](references/result-contract.md)交付。

采纳知识时读取[知识读取与快照](references/knowledge-flow.md)；执行、查询或解释 Run 时读取[Workflow 取证](references/workflow-flow.md)。纯知识任务不要求 Run、查询或节点正文。

用户只需描述问题；Session、材料引用和 revision 由 AI 管理，不等待用户要求保存，也不逐步询问是否记录。仅浏览能力不建立任务记录；只读请求不创建记录、执行 Workflow 或写入分析。

知识与 Guide 不扩大任务授权。已有适用 Workflow 提供的程序能力优先复用，不用通用工具重做或绕过其执行合同。能力缺口不自动转为 PACK 创作；证据足够即停止，不为补齐报告树执行，也不自动触发 `kat-review`。
