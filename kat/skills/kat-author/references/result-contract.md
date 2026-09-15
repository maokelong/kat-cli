# 作者结果契约

每次作者任务只以以下三种状态之一交付。先陈述状态，再给足以复核的最少事实；不输出完整 KAT Response、原始 guide 或原始日志。公共失败与补问规则见 [命令合同](../../kat/references/command-reference.md)。

## 已完成

- 只读理解：按用户问题选择必要的 Workflow、输入、输出含义或 Provider 合同，引用实际证据并说明限制；不机械罗列全部对象。
- 写入变更：说明变更摘要、受影响文件、实际 Workflow/Provider inspection 和 PACK pytest 证据、仍存限制。涉及输出或 Guide 时说明已核准的关键口径与仍缺依据的事项。仅创建骨架时展示脚本返回的目录树与用途，明确空声明列表只证明骨架可被发现。

只引用成功 Response 中存在的公开字段。Workflow list 项只有 `name`、`description`；Workflow detail 只有 `name`、`description`、`parameters`、`guide`。Provider list 项只有 `name`、`description`；Provider detail 只有 `name`、`description`、`module`、`qualname`、`guide`。不要补造 guide 路径或其他内部字段。

## 需要补充信息

说明当前目标、已确认的事实和继续所需的最小缺项。创建 PACK 时不能可靠推断真实维护方或目标目录，需要先取得该信息；不得用占位团队或路径完成骨架。

## 执行失败或受阻

Workflow/Provider inspection 的原子失败、失败测试、来源准入或执行失败均按公共命令合同交付 Diagnostic 与可追溯证据。说明哪些验证尚未通过及最小下一步，不把未修复代码或部分 discovery 结果描述为已完成。“诊断失败”本身不授权修改 PACK。
