---
name: kat-dev-sdk
description: 开发和维护 KAT 官方 SDK 的 Provider、Workflow、公共 Python API、Runtime Guide 与源码侧 API 文档，并构建和验收独立 SDK wheel。用户要求创建或修改 SDK 能力、生成 SDK API 文档、打包 SDK 时使用。
---

# KAT SDK 开发

本 Skill 维护 KAT 源码仓库中的 `kat/sdk/`。使用现成能力进行分析时转到 [kat-analyze](../kat-analyze/SKILL.md)；领域 PACK 开发使用 [kat-author](../kat-author/SKILL.md)。

## 工作流程

1. 定位 SDK 源码仓库，读取仓库工作协议、对应 issue 与设计。明确本次能力、消费者、最小切片和验证方式；不要把已安装的 `site-packages/kat_sdk/` 当作交付源码。
2. 读取 [SDK 开发指南](references/sdk-development.md)，按其中的源码侧复用检查核对 `kat/sdk/providers/`、`workflows/`、`helpers/`、相关测试与真实消费者，并只读作者侧 `base-api.md` 核对框架已有能力。`base-api.md` 缺失时报告文档缺项；本 Skill 不生成或修改它。
3. 实现必要的源码、类型注解、docstring、行为测试和 declaration 直接引用的 Runtime Guide。Provider、Workflow、helper 使用各自既有发现方式，不新增平行导航。
4. 按开发指南从源码静态生成 `kat/sdk/docs/api.md` 与 `kat/sdk/docs/reference/`。以 `_API_MODULES` 和各模块 `__all__` 为唯一 Python API 边界，结合实现、测试和真实消费者形成内容；不 import 或执行 SDK 模块。任一公开符号证据不足时停止正式生成并报告。将生成结果交给人工审核，未经审核不得作为正式 API 文档。
5. 审核完成后向用户提供源码文档路径，由用户自行把 `docs/api.md` 与整个 `docs/reference/` 复制到 `kat-author/references/`。本 Skill、wheel 构建和 Skill 装配都不得自动执行该复制。
6. 按 [SDK 构建与验收流程](references/sdk-build.md) 构建独立 wheel，在隔离部署中验证发现、Runtime Guide、真实使用、升级和卸载。构建不得生成、修改、复制或打包 API 文档。
7. 交付变更位置、SDK 版本、API 文档人工审核状态、wheel/SHA256、实际验证证据和未完成项。是否发布远程资产按用户指令执行。

## 运行环境

调用 KAT 前读取同级 kat 的 [公共命令合同](../kat/references/command-reference.md)，按该合同定位 CLI、Data Home 和相邻 Python；安装依赖遵循 [Python 依赖管理](../kat/references/python-packages.md)。

开发构建使用源码仓库所要求的工具环境，运行验收使用所选 KAT 部署。SDK 独立版本化；CLI、Runtime、Context、装饰器和表工具继续由框架维护。
