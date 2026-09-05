---
name: kat
description: 使用 KAT 发现并执行分析 Workflow、查询 Run Output，或理解、创建、修改、验证和诊断 KAT PACK、Provider 与 Workflow。
---

# KAT

KAT 是唯一面向用户的产品入口。用户用自然语言说明分析目标或 PACK 开发目标，不需要预先选择 CLI 命令。

## 能做什么

- 分析问题：为新分析显式创建 Session，发现并选择 Workflow，按其分析策略执行和查询 Workflow Output。
- 继续已有分析：用 Session ID 与 Run ID 读取当前 Workflow 知识并查询已有输出，或在同一 Session 中显式运行后续 Workflow。
- 临时组合分析：在同一 Session 中组织多个正式 Workflow 并查询各自证据；这些调用各自形成独立根 Run，不创建临时 Workflow、父 Run 或其他编排对象。
- 创作或维护 PACK：分别发现 Workflow 与 Provider 的公开知识，按已有能力实现、修改和测试 PACK。

理解、检查、测试和解释失败默认只读。只有用户明确要求创建、修改或修复时才写入指定 PACK；无匹配 Workflow 不会自动切换为开发任务。

## Agent 路由

只按当前任务渐进加载以下 reference：

1. 每次调用 KAT 前读取 [命令速查](references/command-reference.md)，按其中的平台载荷、Data Home、安全边界、精确命令和 Response 字段执行。
2. 分析问题时读取 [分析流程](references/analysis-flow.md)。只用 Workflow 知识发现分析能力，不扫描 Provider。
3. 创作或维护 PACK 时读取 [PACK 创作流程](references/pack-authoring-flow.md)。Workflow 与 Provider 是两个独立知识入口。
4. 向用户交付前读取 [结果契约](references/result-contract.md)。

不要预先加载未匹配当前任务的流程，也不要从 PACK 文件路径、终端文本或静态能力清单绕过 KAT Response。
