---
name: kat-analyze
description: 使用 KAT 发现并执行已有 Workflow，查询证据、保存分析报告，或仅凭 Session ID 恢复报告树与原依据并继续分析。适用于新分析、恢复已有分析或临时组合多个已有 Workflow 取证。
---

# KAT Analyze

围绕一个用户问题执行已有分析能力，将 Run 解释和总报告保存到所属 Session，供跨任务或上下文缺失时恢复。用户只需描述问题；`kat analysis` 的调用、JSON 请求、材料引用和 revision 由 AI 管理，不等待用户要求保存，也不逐步询问是否记录。

## 定位共享部署

从本 `SKILL.md` 的绝对位置解析相邻 `../kat/`，确认其中的 `SKILL.md`，读取[公共命令合同](../kat/references/command-reference.md)。该 `kat/SKILL.md` 的父目录才是 `<kat-root>`，不使用当前任务目录。每次按合同选择 CLI 绝对路径，沿用当前 Data Home；各 Skill 必须来自同一套部署，缺项时不从其他版本或 `PATH` 拼装。直接使用本 Skill 无需先调用总路由。

## 问题分析流程（必须遵守）

开始时读取[分析流程](references/analysis-flow.md)，在用户已授权的范围内执行以下约束。仅浏览能力不创建记录；只读请求不创建记录、执行 Workflow 或写入分析。已有报告经核对适用即可只读交付，无需重复保存。

| 触发时机 | 必须动作与完成条件 |
|---|---|
| 新任务恢复 Session，或当前缺少继续分析所需信息 | 先 `analysis show` 恢复目标、树、正文和 revision。当前上下文足够时直接继续，不因汇总或上下文压缩本身而机械重读。明确无记录时才依据已知目标和现存 Run 建立记录，损坏不能按无记录覆盖。 |
| 新分析确定目标并选定首个 Workflow | 创建 Session，用首次 `analysis save` 保存目标，成功后再执行。追问和补证沿用该 Session，独立新目标新建 Session。 |
| 准备执行 | 用 `inspect workflow` 获取用途和参数，确认输入后运行。只从 Workflow 知识发现分析能力，不扫描 Provider；Guide 由程序在执行前捕获，无需 AI 提前读取或归档。 |
| 需要解释 Run | 使用其执行时 Guide 和实际 Output：刚执行完直接使用返回内容，缺失的历史或子 Run 信息用 `inspect run` 获取。按需解释子节点，不遍历全部后代；普通分析不拿当前 PACK Guide 替换快照。 |
| 解释形成 | 读取真实查询结果，归档实际采用的关键证据，将必要结论、依据、出处和限制写入一个 `content` 正文并及时 `analysis save`。保存成功后再供后续分析复用；不额外保存选择记录、占位节点或依赖字段。 |
| 形成父解释或总报告 | 优先复用当前上下文中已保存且适用的正文；缺少信息才读取记录。需要子解释时先保存子正文再汇总，树由程序按真实调用关系生成。AI 判断哪些正文需要修订，程序不维护有效性状态。 |
| 交付新报告或修订报告 | 读取[结果契约](references/result-contract.md)，保存总报告正文。成功 Response 即完成保存确认，无需额外回读；交付同一正文的结论及 Session ID。保存未确认时不能承诺新报告可恢复。 |

保存冲突先重读核对，保存响应不确定先确认实际记录；不盲目换 revision 重提或重跑 Workflow。没有匹配 Workflow 时说明能力边界，未经明确授权不转为 PACK 创作。证据足够即结束，不为补齐树继续执行，也不自动触发 `kat-review`。
