---
name: kat
description: KAT 总入口，根据自然语言目标路由到问题分析、PACK 创作维护、分析结论复核、官方 SDK 开发或当前部署的 Python 依赖管理。适用于用户使用 KAT 但尚未指定任务 Skill 的请求。
---

# KAT

按用户目标读取对应入口或共享说明并执行；四个任务 Skill 也可以直接调用。

| 用户目标 | 读取的入口或说明 |
|---|---|
| 分析数据来源、继续已有 Session/Run、查询证据并回答问题 | [kat-analyze](../kat-analyze/SKILL.md) |
| 新增或维护官方 SDK 能力、知识文档，构建 SDK wheel | [kat-dev-sdk](../kat-dev-sdk/SKILL.md) |
| 理解、创建、修改、测试或诊断领域 PACK 及其 Provider、Workflow、领域公共库 | [kat-author](../kat-author/SKILL.md) |
| 总结已完成分析、检查原报告结论与已有证据 | [kat-review](../kat-review/SKILL.md) |
| 查找 SDK Workflow、了解输入输出和调用方式 | [Workflow 导航](references/workflows/index.md) |
| 查找 SDK Provider、了解适用来源和导入方式 | [Provider 导航](references/providers/index.md) |
| 查找和使用 SDK 公共函数 | [公共库导航](references/helpers/index.md) |
| 安装、更新或卸载当前部署的 Python 第三方库 | [Python 依赖管理](references/python-packages.md) |

相对链接以本文件所在目录为基准，先解析为绝对路径，不依赖当前工作目录。只加载当前任务所需入口；目标缺失或会导向实质不同任务时，询问一个最小必要问题。

没有匹配 Workflow 时说明能力缺口，未经用户授权不转入 PACK 创作；分析完成后不自动复核。理解、检查、测试或诊断不自动授权修改 PACK。

## 共享执行边界

五个 Skill 成套安装并随同一版本升级。本目录承载公共 [命令合同](references/command-reference.md)、平台 CLI、相邻私有 Python 与 Bundled PACK；调用 KAT 前按公共合同定位载荷、处理 Data Home，并依据结构化 Response 判断结果。

任务流程和结果要求由对应 Skill 维护。CLI 使用本目录的 Python，依赖管理也使用同一解释器；任务 Skill 不复制环境，也不要求先经过本总路由。
