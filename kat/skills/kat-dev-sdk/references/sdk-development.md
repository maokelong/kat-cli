# 官方 SDK 开发指南

在新增或维护官方 SDK 的 Provider、Workflow、公共 Python API、Runtime Guide 或源码侧 API 文档时读取本指南。开发对象是源码仓库中的 `kat/sdk/`；已安装的 `site-packages/kat_sdk/` 只用于运行验收。

仅安装或升级现成 wheel 时，先读 [Python 依赖管理](../../kat/references/python-packages.md)。CLI 路径、Data Home、参数和 Response 以 [公共命令合同](../../kat/references/command-reference.md) 为准。

## 1. 确定能力归属

SDK 只提供跨 PACK 复用的具体能力。框架 Runtime、Context、装饰器和表工具由 `kat-workflow` 维护；开始实现前只读作者侧 `base-api.md`，不要在本流程生成或修改框架 API 文档。

| 能力 | 源码位置 | 公共发现方式 |
| --- | --- | --- |
| Provider | `providers/<模块>.py` | Python API；`kat inspect provider` 返回 declaration 与 Guide |
| Workflow | `workflows/<领域>/<入口>.py` | `kat inspect workflow --pack kat-sdk`、`kat run`、`ctx.run()` |
| 公共函数 | `helpers/<领域>/<模块>.py` | Python API |

三类能力动手前均执行 SDK 源码侧复用检查：

1. 读取作者侧 `base-api.md`，确认框架已有的公开构建能力；本流程不生成或修改该文件。
2. 检查 `kat/sdk/providers/`、`workflows/`、`helpers/`、`_API_MODULES` 和现有 declaration，确认是否已有同类能力或可组合的公共函数。
3. 检查相关行为测试与真实消费者，确认现有合同和实际用法；旧 Markdown 只能作为辅助证据。
4. 记录复用选择、已有能力不满足需求的具体差距，以及本次最小公共切片。

这是 `kat-dev-sdk` 自己的源码开发流程，不调用作者侧 `reuse-check.md`。只实现已有消费者需要的最小公共切片；测试 fixture 只有经过真实消费验证后才可加入正式 SDK API。

## 2. 组织源码、Guide 与文档

```text
kat/sdk/
├─ pyproject.toml
├─ __init__.py
├─ pack.toml
├─ providers/
├─ workflows/                 # 按领域分目录，无 __init__.py
├─ helpers/                   # 按领域分 Python package
├─ knowledge/                 # wheel 内的 Runtime Guide
│  ├─ providers/
│  └─ workflows/
├─ docs/                      # 源码侧、待人工审核的 API 文档
│  ├─ api.md
│  └─ reference/
└─ tests/
```

Provider 和 Workflow 的 declaration 使用相对 `knowledge/` 的 `guide=` 路径直接定位具体 Guide。`knowledge/` 不设置 `index.md`，也不生成 `*.api.md`。Guide 说明来源数据合同或 Workflow 分析语义，继续进入 wheel。

`docs/` 只保存 Python API 目录与 reference，不进入 wheel。wheel 构建不得生成、修改或复制该目录。

新增 helper 领域包或 Provider 子包时补齐 `__init__.py`，并把包名加入 `pyproject.toml` 的 `tool.setuptools.packages`。Workflow 入口目录不放 `__init__.py`，继续作为 PACK 资源打包。

### Provider

Provider 模块以字面量 `__all__` 声明公开类，通过 `@kat.provider` 声明名称、用途和 Guide。模块同时加入根模块的 `PROVIDER_MODULES` 供 Runtime inspection；如果 Provider 类也是受支持的 Python API，再将模块加入 `_API_MODULES`。

模块导入和 inspection 只加载声明。连接来源、读取业务数据、运行外部解析器放在构造或实际调用阶段。完成标准包括公开 inspection、Guide、真实来源行为和模块行为测试。

### Workflow

每个入口文件定义带 `@kat.workflow` 的 Workflow。名称来自 declaration，目录只组织源码。入口使用框架支持的参数并返回 `None`（无 Outputs）、Table 或非空命名 Table 字典；组合其他 Workflow 时使用 `ctx.run()`。

Workflow 的公共身份来自 declaration、inspection 和 Guide，不加入 `_API_MODULES`，也不生成 Python API reference。

### 公共函数

公共函数位于 `helpers/<领域>/` 的普通 Python 模块，通过字面量 `__all__` 声明公开符号，并提供类型注解、docstring、行为测试和真实消费者。模块加入 `_API_MODULES` 后，消费者按完整模块路径直接导入；公共函数不注册为 Workflow 或 Provider。

## 3. 定义公共 Python API

`kat/sdk/__init__.py` 中私有、有序、字面量形式的 `_API_MODULES` 是 SDK Python API 的唯一模块清单：

```python
_API_MODULES = (
    "kat_sdk.providers.ftrace",
    "kat_sdk.providers.trace_streamer",
    "kat_sdk.helpers.demo.greeting",
)
```

生成顺序严格沿用该元组；每个 reference 的符号顺序严格沿用对应模块的字面量 `__all__`。未登记模块、未由 `__all__` 导出的符号和以下划线开头的成员均视为内部实现，不从可导入性、测试或旧文档反向推断为公共 API。

新增、移除或改变公共 API 行为时更新 SDK 版本。当前字面量公共常量值属于该版本的 API 合同。

## 4. 从源码生成 API 文档

`kat-dev-sdk` 直接读取当前源码 checkout，生成：

```text
kat/sdk/docs/
├─ api.md
└─ reference/
   └─ <每个 _API_MODULES 模块一份 Markdown>
```

生成期间禁止 import 或执行 SDK 模块，也不要求先构建或安装 wheel。按以下步骤执行：

1. 从 `pyproject.toml` 读取 SDK 版本，静态解析 `_API_MODULES`。模块缺失、重复登记或元组无法按字面量读取时停止。
2. 按登记顺序静态解析每个模块的字面量 `__all__`。导出名缺失、重复、以下划线开头或无法解析时停止。
3. 提取公开符号和公开成员的签名、参数种类与顺序、类型、默认值、返回类型、属性和 docstring。递归展开以下划线命名的私有类型别名；无法静态展开时停止，不能把私有类型名写入文档。
4. 结合实现、行为测试和真实消费者提炼一句话用途、稳定错误、重要边界与副作用。测试和旧文档只能为已由 `__all__` 确认的符号提供证据，不能扩大 API 面。
5. 示例只从已有测试或真实消费者收敛；没有可验证示例时省略。不要发明示例、行为或错误合同。
6. 任一导出符号缺少足够证据时列出该符号并停止正式生成；不得静默遗漏，也不得以“待补充”占位。

`api.md` 只写标题、SDK wheel 版本，以及按模块排列的公开导入路径、一句话用途和 reference 相对链接。每个 reference 写模块路径、一句话用途、该模块全部公开符号的签名、参数、返回值、公开错误、必要边界或副作用，以及可选的经验证示例。不要复制架构说明或 Runtime Guide 的来源关系、Schema 和领域解释。

错误只记录调用方可识别的公开异常类型与稳定触发条件；不固定异常消息，也不穷举 Python、文件系统或第三方库可能产生的全部低层错误。公开常量记录完整导入路径、当前字面值和用途。

## 5. 人工审核与作者侧交付

生成文件首先留在 `kat/sdk/docs/`。人工逐项核对版本、模块和符号顺序、签名、证据、链接以及内容是否精简准确；未经审核的结果不是正式 API 文档。

审核完成后，由用户自行复制：

```text
kat/sdk/docs/api.md          -> kat/skills/kat-author/references/api.md
kat/sdk/docs/reference/      -> kat/skills/kat-author/references/reference/
```

`kat-dev-sdk` 不写入 `kat-author`，wheel 构建和 Skill 装配也不自动同步。框架侧 `base-api.md` 由框架开发流程维护，本 Skill 始终只读。

## 6. 构建与运行验收

按 [SDK 构建与验收流程](sdk-build.md) 构建 wheel、检查摘要与内容，并在真实 KAT 环境验证安装、升级和卸载。API 文档与 wheel 使用同一源码版本号，但不属于 wheel 内容；构建验收只检查它们未被打包。
