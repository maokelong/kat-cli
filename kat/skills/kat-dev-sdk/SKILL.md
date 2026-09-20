---
name: kat-dev-sdk
description: 开发和维护 KAT 官方 SDK 的 Provider、Workflow、领域公共库及知识文档，构建 SDK wheel 并验证独立安装升级。用户要求创建 SDK 能力、修改公共库或 SDK 文档、打包 SDK 时使用。
---

# KAT SDK 开发

本 Skill 维护 KAT 源码仓库中的 `kat/sdk/`，交付具体公共能力与知识。使用现成能力进行分析时转到 [kat-analyze](../kat-analyze/SKILL.md)；领域 PACK 开发使用 [kat-author](../kat-author/SKILL.md)。

## 工作流程

1. 确认用户要修改的 SDK 源码仓库，读取仓库工作协议及已有 issue/设计。明确目标能力、领域目录、消费者和验证方式；仅有已安装部署时，先取得源码位置，不直接修改 site-packages 作为交付。
2. 读取 [SDK 开发指南](references/sdk-development.md)，选择 Provider、Workflow 或公共库分支。Workflow 和公共库均按领域分目录，使用框架公开 API。新增函数库或函数前执行 [公共库复用检查](../kat/references/libraries/index.md#新开发前的复用检查)，满足需求时直接复用，有缺口时说明依据再实现必要部分。
3. 同步实现、类型注解、docstring 和 SDK 知识；按开发指南在 kat 的 `references/workflows/`、`references/providers/` 或 `references/libraries/` 新增或更新介绍与分类导航。完成标准是 wheel 的源码与随包知识、Skill 的使用介绍均齐全，名称、导入路径及适用版本一致。
4. 按 [SDK 构建与验收流程](references/sdk-build.md) 构建独立 wheel，在测试部署中验证知识读取、真实使用及升级。复验未安装和卸载 SDK 后，原有框架与不依赖 SDK 的 PACK 仍可用。
5. 交付代码与文档位置、SDK 版本、wheel/SHA256、实际测试证据和未完成项。是否发布远程资产按用户指令执行。

## 运行环境

调用 KAT 前读取同级 kat 的 [公共命令合同](../kat/references/command-reference.md)，按该合同定位 CLI、Data Home 和相邻 Python；安装依赖遵循 [Python 依赖管理](../kat/references/python-packages.md)。

开发构建使用源码仓库所要求的工具环境，运行验收使用所选 KAT 部署。SDK 独立版本化，CLI、Runtime、Context、装饰器和表工具继续由框架维护。
