---
status: accepted
---

# API 文档作为经审核的作者资料独立于 wheel 交付

Workflow 作者需要在实现前快速判断框架与官方 SDK 是否已经提供公共能力。构建期从部分源码渲染 `knowledge/**/*.api.md` 并将其放入 SDK wheel，既无法在交付前人工审核，也没有覆盖明确登记的全部 Python API；把 helper、Provider 和 Workflow 再复制成三套 Skill 导航还会与源码、inspection 和 Guide 形成平行合同。

框架与 SDK 的 API 文档分别绑定各自 wheel 版本，但不进入任何 wheel。框架开发流程维护 `kat-author/references/base-api.md`，以 `kat.__all__` 和 `kat.dataprovider.__all__` 为公共边界。`kat-dev-sdk` 从 SDK 源码静态生成 `kat/sdk/docs/api.md` 与逐模块的 `kat/sdk/docs/reference/`；生成结果是受版本控制、等待人工审核的交付源码。生成不得导入或执行 SDK，公开模块来自 SDK 的有序登记清单，公开符号及顺序来自各模块的字面量 `__all__`。无法从源码、测试和真实消费者取得足够证据时停止生成正式稿，不静默遗漏或猜测合同。

SDK 源码侧文档接受人工审核后，由用户手工复制到 `kat/skills/kat-author/references/api.md` 与 `kat/skills/kat-author/references/reference/`。构建脚本、wheel 构建和 Skill 装配都不自动完成复制。`kat-author` 默认信任当前提供的文档已经与对应 wheel 配对，不读取 distribution metadata 做运行时版本比较；新增 Workflow 或修改其行为、输入、输出、依赖和数据处理步骤时，依次查阅 `base-api.md`、完整的短 `api.md` 和候选模块 reference，并记录实际复用结论。原有 `kat/skills/kat/references/helpers/`、`providers/`、`workflows/` 不再是作者侧公共 API 入口。

Python API 文档只描述公开导入能力。Provider 类可以作为公开 Python API 进入 reference；Provider 的来源关系、Schema、限制和诊断仍由 declaration 关联的 Guide 说明。Workflow 的公共身份继续来自 declaration、inspection 与 Guide，不把 Workflow 函数登记为第二套 Python API。作者使用 `kat inspect workflow` 和 `kat inspect provider` 核对当前安装版本，不用 API 文档替代 inspection，也不回查 SDK 源码寻找内部符号。

SDK wheel 继续携带 declaration 直接引用的 `knowledge/providers/` 与 `knowledge/workflows/` Guide。`knowledge/index.md` 没有 Runtime 消费者，因此删除；API 文档目录、旧的 `api.md`/`reference` 布局及 `knowledge/**/*.api.md` 均不得进入 wheel。wheel 构建只验证 Runtime Guide 与安装资源，不生成、审核或复制 API 文档。

本决定局部替代 [ADR-0085](0085-official-capabilities-ship-as-an-independent-sdk.md) 中 `references/helpers/` 作为公共函数文档入口、Griffe/Griffe2MD 在构建期生成 API Markdown，以及生成文档随 SDK wheel 交付的部分。ADR-0085 关于独立 SDK、公共 PACK、Provider/Workflow 发现与 inspection、Guide 随 SDK 版本化及真实安装升级验证的决定继续有效。完整生成、使用与验证规则见 [Workflow 作者 API 文档 SDD](../specs/workflow-authoring-api-discovery.md) 和 [Issue #300](https://github.com/maokelong/kat-cli/issues/300)。
