---
name: kat
description: KAT 总入口，根据自然语言目标路由到问题分析、PACK 创作维护、分析结论复核、官方 SDK 开发或当前部署的 Python 依赖管理。适用于用户使用 KAT 但尚未指定任务 Skill 的请求。
---

# KAT

按用户目标读取对应入口或共享说明并执行；四个任务 Skill 也可以直接调用。

| 用户目标 | 读取的入口或说明 |
|---|---|
| 应用 PACK 知识、分析数据、运行 Workflow、凭 Session ID 恢复或继续任务、查询证据，或明确管理已有 Session | [kat-analyze](../kat-analyze/SKILL.md) |
| 新增或维护官方 SDK 能力、知识文档，构建 SDK wheel | [kat-dev-sdk](../kat-dev-sdk/SKILL.md) |
| 固化经验知识，理解、创建、修改、测试或诊断 PACK 的知识入口、Provider、Workflow、领域公共库 | [kat-author](../kat-author/SKILL.md) |
| 总结已完成分析、检查原报告结论与已有证据 | [kat-review](../kat-review/SKILL.md) |
| 安装、更新或卸载当前部署的 Python 第三方库 | [Python 依赖管理](references/python-packages.md) |

相对链接以本文件所在目录为基准，先解析为绝对路径，不依赖当前工作目录。只加载当前任务所需入口；目标缺失或会导向实质不同任务时，询问一个最小必要问题。

没有匹配知识入口且没有匹配 Workflow 时说明能力缺口，未经用户授权不转入 PACK 创作；分析完成后不自动复核。理解、检查、测试或诊断不自动授权修改 PACK。

## 共享执行边界

五个 Skill 成套安装并随同一版本升级。本目录承载 [共享执行合同](references/command-reference.md)、平台 CLI、相邻私有 Python 与 Bundled PACK。调用前读取共享合同取得部署、PACK 发现与返回规则；所需命令用法和结果要求直接由任务入口提供。

任务流程和结果要求由对应 Skill 维护。CLI 使用本目录的 Python，依赖管理也使用同一解释器；任务 Skill 不复制环境，也不要求先经过本总路由。
