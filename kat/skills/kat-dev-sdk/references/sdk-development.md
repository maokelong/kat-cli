# 官方 SDK 开发指南

在新增或维护官方 SDK 的 Provider、Workflow、公共 Python 函数，以及修改随包知识、构建 SDK wheel 时读取本指南。开发对象是 KAT 源码仓库中的 `kat/sdk/`；已安装的 `site-packages/kat_sdk/` 用于运行和验收，源码修改后通过重新构建、安装 wheel 生效。

仅安装或升级现成 wheel 时，先读 [Python 依赖管理](../../kat/references/python-packages.md)。CLI 路径、Data Home、参数和 Response 以 [公共命令合同](../../kat/references/command-reference.md) 为准。

## 可运行的 demo

SDK 0.1.1 提供一组同领域示例：`helpers/demo/greeting.py` 的 `build_greeting()` 生成问候语，`workflows/demo/greeting.py` 的 `demo-greeting` 调用它并返回一行 `message` 表。阅读 kat Skill 的 [公共库介绍](../../kat/references/helpers/demo/greeting.md) 和 [Workflow 介绍](../../kat/references/workflows/demo/greeting.md)，再核对当前 SDK 源码中的 Workflow Guide 及装饰器关联。`kat inspect workflow --pack kat-sdk --workflow demo-greeting` 读取用途和参数；Guide 在执行后从 Run 结果或 `inspect run` 读取。

源码测试位于 `kat/sdk/tests/test_demo_greeting.py`，完整安装验收还验证 CLI 执行、结果查询及跨 PACK 调用。以此为最小例子学习目录、声明、知识、打包和运行链路，再添加具体领域能力。

## 1. 确定能力归属

SDK 只提供跨领域复用的具体 Provider、Workflow、公共函数及知识。框架的 Runtime、Context、装饰器和表工具继续由框架维护。SDK 是可选包：未安装或卸载后，框架及不依赖它的 PACK 仍可运行；安装 SDK 只增加能力。

在源码仓库中先按仓库协议记录问题、最小切片和验收方式，再选择一种交付：

| 能力 | 源码位置 | 使用及发现方式 |
| --- | --- | --- |
| Provider | `providers/<模块>.py` | Python 导入；`kat inspect provider` 读取公共声明与 Guide |
| Workflow | `workflows/<领域>/<入口>.py` | `kat inspect workflow --pack kat-sdk`、`kat run`、`ctx.run()` |
| 公共函数 | `helpers/<领域>/<模块>.py` | Python 直接导入；AI 从 kat 公共库导航找到用法 |

三类能力动手前均执行 [开发前复用检查](../../kat-author/references/reuse-check.md)，确认是否已有同类实现；开发 Workflow/Provider 时，还须检查 helpers 中的公共函数能否支撑所需步骤。检查 SDK 源码中的 `workflows/`、`providers/`、`helpers/` 及相关领域 PACK，记录复用选择与具体缺口。当前正式 SDK 的具体内容以源码和当前安装版本为准；示例与测试 fixture 只有经过实际消费者验证后才晋升为正式能力。

## 2. 组织源码和知识

```text
kat/sdk/
├─ pyproject.toml
├─ __init__.py
├─ pack.toml
├─ providers/
├─ workflows/                 # 按领域分目录
├─ helpers/                 # 按领域分目录
├─ knowledge/
│  ├─ index.md
│  ├─ providers/
│  └─ workflows/
└─ tests/
```

`workflows/` 和 `helpers/` 均先按领域分目录，例如 `workflows/memory/summarize.py`、`helpers/memory/units.py`；Workflow Guide 位于 `knowledge/workflows/memory/`；公共库的使用说明与 API Markdown 全部位于 `kat/skills/kat/references/helpers/memory/`，SDK 不生成或携带公共库 Markdown。公共库的父包与领域包均设置 `__init__.py`，并显式登记 `kat_sdk.helpers`、`kat_sdk.helpers.memory` 等包名；Workflow 目录保持无 `__init__.py`。

安装后资源根为 `kat_sdk/`，直接包含 `pack.toml`，不增加 `packs/` 层。框架将该根目录补充到既有 PACK 发现范围，沿用目录去重和同名冲突规则；无需复制到各领域 PACK 或 Data Home。

`pyproject.toml` 当前显式列出 Python packages。新增 `helpers/<领域>/` 或 Provider 子包时，添加对应 `__init__.py`，并将包名纳入 `tool.setuptools.packages`。Workflow 入口目录沿用 PACK 规则，不放 `__init__.py`，通过 `workflows/**/*.py` 资源模式打包。额外运行资源也须显式纳入 package-data。以 wheel 内容确认打包结果，不能只检查源码目录。

### Provider

参考已有 Provider 实现，用框架的公开接口创建和查询标准表。模块以 `__all__` 列出公开 API，并声明 Provider 名称、摘要和 Guide：

```python
from kat import provider

__all__ = ["ExampleProvider"]

@provider(
    name="example-source",
    description="描述来源及其用途。",
    guide="providers/example-source.md",
)
class ExampleProvider:
    """在实际实现中说明初始化参数、来源格式和查询接口。"""
```

上例仅展示声明；交付时须实现真实来源行为并测试。Guide 位于 `knowledge/providers/example-source.md`，路径相对于 `knowledge/`。在 SDK 根 `__init__.py` 的 `PROVIDER_MODULES` 元组中加入完整模块名，例如 `kat_sdk.providers.example`。

在 `kat/skills/kat/references/providers/<Provider名称>.md` 新增或更新使用介绍，并更新 [Provider 导航](../../kat/references/providers/index.md)。介绍写明适用 SDK 版本、来源类型、依赖、导入路径、最小调用示例、输出和限制，以及读取当前 Guide 的 inspection 命令。

模块导入和 inspection 只加载声明与知识；连接来源、读取业务数据、运行外部解析器放在实际调用阶段。完成标准是公共列表及详情可读，并且真实来源查询返回预期数据。

### Workflow

每个入口文件定义一个带 `@kat.workflow` 的 Workflow，声明名称、用途和业务参数说明；需要分析 Guide 时指向 `workflows/<领域>/<主题>.md`。名称来自装饰器，目录只负责组织源码。

在 `kat/skills/kat/references/workflows/<领域>/<入口>.md` 新增或更新使用介绍，并更新 [Workflow 导航](../../kat/references/workflows/index.md)。介绍写明适用 SDK 版本、PACK/Workflow 名称、用途、输入、输出、最小运行示例、错误边界及 inspection 命令。

入口接收框架 `kat.Context`，返回框架支持的 Table 或非空命名 Table 字典。复用 Provider 和普通函数时从 `kat_sdk` 导入；组合其他 Workflow 时使用：

```python
result = ctx.run("kat-sdk", "目标-workflow", **inputs)
```

`ctx.run()` 返回只读 Catalog；需要将子调用结果作为当前 Workflow 输出时，先用框架表工具查询为 Table，再返回。

不通过 import 其他入口函数绕过框架执行。完成标准是列表、参数、Guide、直接执行和嵌套执行指向同一份能力；零 Workflow 的 SDK 也是合法公共 PACK。

### 公共函数

按第 1 节完成复用检查后，明确公共函数服务的 Workflow/Provider 及其职责；已有函数满足需求时直接导入，只为实际消费者补齐必要缺口。

公共库介绍维护在 `kat/skills/kat/references/helpers/<领域>/<模块>.md`，并更新该目录的 `index.md` 导航。文档说明适用 SDK 版本、导入方式、函数签名、参数、返回值、异常和示例；SDK 中不设置 `knowledge/helpers/`。

在 `helpers/<领域>/` 中编写普通 Python 模块，通过 `__all__` 声明公开函数并提供类型注解、docstring 和行为测试。消费者按模块导入：

```python
from kat_sdk.helpers.<领域>.<模块> import <函数>
```

以上是路径占位示意，替换为实际标识符。公共函数不注册为 Workflow 或 Provider，也不增加 CLI 发现命令。AI 从 kat Skill 的 [公共库导航](../../kat/references/helpers/index.md) 阅读方法介绍，再核对当前 SDK 版本，必要时读取已安装函数签名与 docstring。

## 3. 写 API 文档与使用知识

| 内容 | 维护位置 |
| --- | --- |
| 参数及返回类型 | Python 类型注解 |
| 单位、语义、限制、异常、示例 | Python docstring，使用 Google 风格 |
| Provider/Workflow API Markdown | 构建时自动生成 `knowledge/<类别>/<领域>/<模块>.api.md`（Provider 按实际模块路径） |
| 公共库 API Markdown | 与使用说明一起维护在 kat Skill 的 `references/helpers/<领域>/` |
| Workflow/Provider 使用介绍 | kat Skill 的 `references/workflows/<领域>/`、`references/providers/` |
| 来源数据合同、分析解释 | SDK 中由装饰器引用的 Guide |
| 能力用途及文档入口 | 手写 `knowledge/index.md` |

新增、修改或移除能力时同步更新对应介绍及分类导航，检查所有相对链接。Skill 介绍负责选择能力和最小用法；Provider/Workflow 的详细 Guide 与生成 API 仍随 SDK 交付。通过 Provider inspection 核对当前来源知识，Workflow inspection 核对用途与参数，解释结果时采用该 Run 保存的 Guide，避免复制整份数据合同。公共库全部 Markdown 继续只随 Skill 交付。

构建工具静态解析源码，不执行业务模块。为公开类、方法和函数写清楚参数、返回值与异常；注解不能替代单位、时钟域和数据含义。

Provider/Workflow 生成的 `*.api.md` 进入 wheel，源码中不维护同名文件。手写 Guide 使用独立文件名；Provider 必须声明非空 Guide，Workflow 可按需要声明。更新首页导航，使用知识目录内部的相对链接；构建会拒绝缺失的 Guide、越界或失效的本地链接，以及覆盖手写文件的行为。

在当前部署中定位知识首页：

```text
<bundled-python> -I -B -c "from importlib.resources import files; print(files('kat_sdk').joinpath('knowledge/index.md'))"
```

`<bundled-python>` 按 Python 依赖管理文档解析为当前 KAT 相邻解释器的绝对路径。此命令要求 SDK 已安装；缺席时不应把它解释为框架故障。

## 4. 构建与安装验收

按 [SDK 构建与验收流程](sdk-build.md) 准备环境、运行构建脚本、检查 wheel 与 SHA256，并在真实 KAT 环境验证安装、升级和卸载。该指南也说明产物目录、常见构建问题及交付证据。
