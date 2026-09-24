---
name: kat-analyze
description: 使用 KAT 发现并执行已有 Workflow，查询 Run Output 并回答性能分析问题。适用于新分析、继续已有 Session/Run 或临时组合多个已有 Workflow 取证。
---

# KAT Analyze

根据用户问题执行已有分析能力，依据可追溯证据形成结论。

## 定位共享部署

从本 `SKILL.md` 的绝对位置解析相邻 `../kat/`，确认其中的 `SKILL.md`，读取 [公共命令合同](../kat/references/command-reference.md)。该 `kat/SKILL.md` 的父目录才是公共合同中的 `<kat-root>`；不要把当前任务目录或工作目录当作载荷根。

每次调用按公共合同选择 CLI 的绝对路径，直接沿用当前 Data Home，不主动询问是否修改目录；按 Response 与失败边界交付。四个 Skill 必须来自同一套部署；缺少共享目录或载荷时说明部署缺项，不从其他版本或 `PATH` 拼装。直接使用本 Skill 无需先调用总路由。

## 分析硬约束

新分析必须依次完成以下阶段：

1. 根据用户目标发现并选定 Workflow。
2. 读取所选 Workflow 的公开参数与分析合同。
3. 根据合同准备输入；可以按需读取、提取、转换或组合用户提供的材料。
4. 执行 Workflow，并取得回答问题所需的 Run Output 证据；必要时按分析流程检查相关子 Run。
5. 基于 Workflow Guide（如有）与查询证据形成结论。

不得跳过、颠倒或绕过上述阶段。输入准备只用于满足所选 Workflow 的参数合同，不能代替 Workflow 完成分析。

没有匹配 Workflow、Workflow inspection 失败或输入无法满足合同时，停止并说明能力边界或请求最小必要信息；不得改用通用工具自行完成分析。

已有 Session/Run 从对应 Workflow 合同与既有证据继续，不重新执行新分析的全局发现流程。

## 分析与交付

1. 读取 [分析流程](references/analysis-flow.md)，确认问题和新分析或已有 Session/Run 起点，渐进选择 Workflow、执行并查询最少证据。
2. 只从 Workflow 知识发现分析能力，不扫描 Provider，不把输入路径或日志当作成功结果。
3. 没有匹配 Workflow 时说明已发现的能力边界；未经用户明确授权，不切换到 PACK 创作或修改源码。
4. 交付前读取 [分析结果契约](references/result-contract.md)，区分事实、推断和不确定性。证据足够即结束，不自动触发 `kat-review`。
