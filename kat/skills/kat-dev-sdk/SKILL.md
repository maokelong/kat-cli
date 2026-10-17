---
name: kat-dev-sdk
description: 开发官方 SDK 能力、生成源码侧 API 文档，或从现有源码构建和验收 SDK wheel。按用户指定目标选择流程。
---

# KAT SDK 开发

维护源码仓库的 `kat/sdk/`。**必须按用户目标选择下列流程，保留所选流程的必要步骤。** 单独构建 wheel 不以文档生成、人工审核或作者侧复制为前置条件。

调用 KAT 前读取同级 kat 的 [共享执行合同](../kat/references/command-reference.md)，从本文件绝对位置解析共享部署，使用相邻 CLI/Python。源码资源从任务已明确的 `<repository-root>` 定位；缺仓库位置时询问该位置，不全局搜索，不把已安装 `site-packages/kat_sdk/` 当作交付源码。构建使用仓库要求的工具环境，运行验收使用选定部署。

## 1. 选择当前目标

| 目标 | 流程与材料 |
| --- | --- |
| 新增或修改 Provider、Workflow、公共函数、Runtime Guide | 按下一节能力开发；公共 API 变化再读文档流程，交付 wheel 时再读构建流程 |
| 只生成或更新 API 文档 | 读取 [API 文档生成与交付](references/api-documentation.md)，不要求制造源码改动或构建 wheel |
| 从现有源码构建、检查或验收 wheel | 直接读取 [SDK 构建与验收](references/sdk-build.md)，不要求重做完整开发流程 |

使用现成能力分析时转到 [kat-analyze](../kat-analyze/SKILL.md)，开发用户领域 PACK 时使用 [kat-author](../kat-author/SKILL.md)。SDK 独立版本化，框架 Runtime、Context、装饰器和表工具仍由 `kat-workflow` 维护。

## 2. 能力开发

开始前读取仓库工作协议、对应 issue 与设计，明确消费者、最小切片和验证方式。只读相邻作者侧 [框架 API](../kat-author/references/base-api.md)，缺失时报告文档缺项；本 Skill 不生成或修改它。

开发前按源码核对复用能力：检查 `kat/sdk/providers/`、`workflows/`、`helpers/`、`_API_MODULES` 与声明，结合行为测试及真实消费者确认合同。旧 Markdown 只作辅助证据。记录选中能力或实际缺口，只实现已有消费者需要的最小公共切片，不调用作者侧的文档复用流程。

| 能力 | 源码与发现 |
| --- | --- |
| Provider | `providers/`；公开 Python API 和 `kat inspect provider` |
| Workflow | `workflows/<领域>/`；`kat inspect workflow --pack kat-sdk`，通过 run 或 `ctx.run()` 使用 |
| 公共函数 | `helpers/<领域>/` 的普通 Python 模块；经登记的公开 Python API |

新增 helper 领域包或 Provider 子包时补齐 `__init__.py` 和 `pyproject.toml` 的 `tool.setuptools.packages`。Workflow 入口目录不放 `__init__.py`，作为 PACK 资源。Provider 的根 `PROVIDER_MODULES` 用于 Runtime 发现；公开 Python 模块另外登记 `_API_MODULES` 并以字面量 `__all__` 声明符号，不能用可导入性扩大公共 API。Workflow 不登记成 Python API。公开 API 变更更新 SDK 版本。

declaration 的 `guide=` 相对 SDK 的 `knowledge/`，Provider/Workflow Guide 随 wheel 交付；不建立 `knowledge/index.md` 或 `*.api.md` 导航。用途、输入与执行前提写在声明；Workflow Guide 解释结果，执行后按 Run 快照取得，Provider Guide 从 detail 取得。API 文档只描述公开导入能力，不复制来源 Schema 与领域解释。

模块导入和 inspection 只加载声明，连接来源、查询、解析与外部进程放在构造或实际调用。实现类型注解、docstring、行为测试、实际消费者和必要 Guide。fixture 只有经真实消费验证才加入正式公共 API。

## 3. 验证当前切片

按变更执行源码测试和真实消费验证；Provider/Workflow 声明变更需相应公开 inspection，Python API 需隔离导入与行为验证。命令使用同套部署：

```text
kat inspect provider
kat inspect provider --provider V
kat inspect workflow --pack kat-sdk
kat inspect workflow --pack kat-sdk --workflow W
```

公开调用路径以登记 API 为准，来源语义读 Provider Guide，Workflow detail 提供输入合同，不为回读 Guide 自动运行 Workflow。涉及组合或来源能力时按真实目标验证，不用列表成功代替行为证据。

API 文档按[生成流程](references/api-documentation.md)从源码静态产生，经人工审核后由用户手工复制到作者资料；Skill、wheel 构建和装配都不自动复制。SDK 升级供程序作者使用时说明需同步对应审核文档，不声称安装成功就已更新资料，也不增加运行时版本比较。

涉及 wheel 交付时按[构建与验收](references/sdk-build.md)执行。文档不进入 wheel，构建不生成、修改或安装文档。需要管理现成依赖时遵循 [Python 依赖管理](../kat/references/python-packages.md)。

## 4. 交付

按用户目标交付实际改动、SDK 版本、复用结论、验证证据与限制。涉及文档才说明生成位置、人工审核与复制状态；涉及构建才提供 wheel、SHA256 与已验证平台。不把未审核文档、未执行验证或未完成验收描述为完成。远程发布依用户授权，不因本地构建自动发布。
