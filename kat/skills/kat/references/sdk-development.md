# 官方 SDK 开发指南

在新增或维护官方 SDK 的 Provider、Workflow、公共 Python 函数，以及修改随包知识、构建 SDK wheel 时读取本指南。开发对象是 KAT 源码仓库中的 `kat/sdk/`；已安装的 `site-packages/kat_sdk/` 用于运行和验收，源码修改后通过重新构建、安装 wheel 生效。

仅安装或升级现成 wheel 时，先读 [Python 依赖管理](python-packages.md)。CLI 路径、Data Home、参数和 Response 以 [公共命令合同](command-reference.md) 为准。

## 1. 确定能力归属

SDK 只提供跨领域复用的具体 Provider、Workflow、公共函数及知识。框架的 Runtime、Context、装饰器和表工具继续由框架维护。SDK 是可选包：未安装或卸载后，框架及不依赖它的 PACK 仍可运行；安装 SDK 只增加能力。

在源码仓库中先按仓库协议记录问题、最小切片和验收方式，再选择一种交付：

| 能力 | 源码位置 | 使用及发现方式 |
| --- | --- | --- |
| Provider | `providers/<模块>.py` | Python 导入；`kat inspect provider` 读取公共声明与 Guide |
| Workflow | `workflows/<领域>/<入口>.py` | `kat inspect workflow --pack kat-sdk`、`kat run`、`ctx.run()` |
| 公共函数 | `libraries/<领域>/<模块>.py` | Python 直接导入；AI 从知识首页找到用法 |

先确认已有能力是否能满足需求。当前正式 SDK 的具体内容以源码和当前安装版本为准；示例与测试 fixture 只有经过实际消费者验证后才晋升为正式能力。

## 2. 组织源码和知识

```text
kat/sdk/
├─ pyproject.toml
├─ __init__.py
├─ pack.toml
├─ providers/
├─ workflows/                 # 按领域分目录
├─ libraries/                 # 按领域分目录
├─ knowledge/
│  ├─ index.md
│  ├─ providers/
│  ├─ workflows/
│  └─ libraries/
└─ tests/
```

`workflows/` 和 `libraries/` 均先按领域分目录，例如 `workflows/memory/summarize.py`、`libraries/memory/units.py`；对应知识位于 `knowledge/workflows/memory/` 和 `knowledge/libraries/memory/`。公共库的父包与领域包均设置 `__init__.py`，并显式登记 `kat_sdk.libraries`、`kat_sdk.libraries.memory` 等包名；Workflow 目录保持无 `__init__.py`。

安装后资源根为 `kat_sdk/`，直接包含 `pack.toml`，不增加 `packs/` 层。框架将该根目录补充到既有 PACK 发现范围，沿用目录去重和同名冲突规则；无需复制到各领域 PACK 或 Data Home。

`pyproject.toml` 当前显式列出 Python packages。新增 `libraries/<领域>/` 或 Provider 子包时，添加对应 `__init__.py`，并将包名纳入 `tool.setuptools.packages`。Workflow 入口目录沿用 PACK 规则，不放 `__init__.py`，通过 `workflows/**/*.py` 资源模式打包。额外运行资源也须显式纳入 package-data。以 wheel 内容确认打包结果，不能只检查源码目录。

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

模块导入和 inspection 只加载声明与知识；连接来源、读取业务数据、运行外部解析器放在实际调用阶段。完成标准是公共列表及详情可读，并且真实来源查询返回预期数据。

### Workflow

每个入口文件定义一个带 `@kat.workflow` 的 Workflow，声明名称、用途和业务参数说明；需要分析 Guide 时指向 `workflows/<领域>/<主题>.md`。名称来自装饰器，目录只负责组织源码。

入口接收框架 `kat.Context`，返回框架支持的 Table/Catalog。复用 Provider 和普通函数时从 `kat_sdk` 导入；组合其他 Workflow 时使用：

```python
result = ctx.run("kat-sdk", "目标-workflow", **inputs)
```

不通过 import 其他入口函数绕过框架执行。完成标准是列表、参数、Guide、直接执行和嵌套执行指向同一份能力；零 Workflow 的 SDK 也是合法公共 PACK。

### 公共函数

在 `libraries/<领域>/` 中编写普通 Python 模块，通过 `__all__` 声明公开函数并提供类型注解、docstring 和行为测试。消费者按模块导入：

```python
from kat_sdk.libraries.<领域>.<模块> import <函数>
```

以上是路径占位示意，替换为实际标识符。公共函数不注册为 Workflow 或 Provider，也不增加 CLI 发现命令。AI 通过 `knowledge/index.md` 中的链接阅读方法用法。

## 3. 写 API 文档与使用知识

| 内容 | 维护位置 |
| --- | --- |
| 参数及返回类型 | Python 类型注解 |
| 单位、语义、限制、异常、示例 | Python docstring，使用 Google 风格 |
| API Markdown | 构建时自动生成 `knowledge/<类别>/<领域>/<模块>.api.md`（Provider 按实际模块路径） |
| 来源说明、使用流程、分析解释 | 手写 Guide 或教程 |
| 能力用途及文档入口 | 手写 `knowledge/index.md` |

构建工具静态解析源码，不执行业务模块。为公开类、方法和函数写清楚参数、返回值与异常；注解不能替代单位、时钟域和数据含义。

生成的 `*.api.md` 进入 wheel，源码中不维护同名文件。手写 Guide 使用独立文件名；Provider 必须声明非空 Guide，Workflow 可按需要声明。更新首页导航，使用知识目录内部的相对链接；构建会拒绝缺失的 Guide、越界或失效的本地链接，以及覆盖手写文件的行为。

在当前部署中定位知识首页：

```text
<bundled-python> -I -B -c "from importlib.resources import files; print(files('kat_sdk').joinpath('knowledge/index.md'))"
```

`<bundled-python>` 按 Python 依赖管理文档解析为当前 KAT 相邻解释器的绝对路径。此命令要求 SDK 已安装；缺席时不应把它解释为框架故障。

## 4. 构建独立 wheel

在 KAT 源码仓库根目录，用满足 `kat/sdk/pyproject.toml` 要求的开发 Python 执行：

```text
python -m pip install build==1.6.1
python build/build_sdk_wheel.py --output target/sdk-wheel
```

输出目录必须尚不存在；重建时选择新的目录。脚本在临时目录生成知识，构建并校验 wheel，输出：

```text
target/sdk-wheel/
├─ kat_sdk-<版本>-py3-none-any.whl
└─ kat_sdk-<版本>-py3-none-any.whl.sha256
```

版本来自 SDK 的 `pyproject.toml`，独立于框架版本。依赖范围须由实际兼容验证支持。检查 wheel 中有新增实现、声明、Guide、API MD 和导航，且不含测试与构建工具；没有正式能力的类别无需放置空目录。

## 5. 在真实 KAT 环境安装和验收

优先使用隔离的测试部署。保持当前 CLI、框架和原生依赖不变，在其相邻 Python 中安装已构建 wheel：

```text
<bundled-python> -m pip install --no-deps --upgrade <SDK-wheel绝对路径>
<bundled-python> -m pip show kat-sdk
<bundled-python> -m pip check
```

这里的 `--no-deps` 用于已有兼容依赖的 SDK 独立升级验收；它不解决依赖缺失或版本冲突。若 `pip check` 失败，先准备满足 SDK 元数据要求的受支持 KAT 部署。PowerShell 调用带引号的解释器路径时加 `&`。

按新增能力执行 inspection、实际调用和行为测试：

```text
kat inspect
kat inspect provider
kat inspect provider --provider <名称>
kat inspect workflow --pack kat-sdk
kat inspect workflow --pack kat-sdk --workflow <名称>
kat session create
kat run --session <id> --pack kat-sdk --workflow <名称> -- <业务参数>
kat test --pack-dir <已安装kat_sdk目录的绝对路径>
```

最后一条只在候选 PACK 配有测试时使用；正式 SDK wheel 不携带 `tests/`，不能把空测试集当成验证。SDK 源码测试用同一宿主的 `-I -B -m pytest <仓库>/kat/sdk/tests -q` 运行。公共函数另行验证隔离导入、返回值和知识导航。

源码仓库提供完整安装及升级验收脚本：

```text
python build/verify_sdk_install.py --kat <当前CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output <新的验收目录>
```

脚本在独立环境中验证未安装、安装、升级、卸载，使用仅测试用能力检查发现和知识更新。它不能替代新增业务能力的专门测试。交付须覆盖：

- 新能力的发现、知识读取与真实执行；需要外部工具的能力明确记录实际工具和样本。
- SDK 升级后旧能力仍可使用，CLI 与框架版本保持不变。
- 未安装及卸载后，原有 PACK 的发现和不依赖 SDK 的执行继续可用。
- 当前支持的 Windows/Linux 环境分别验收；纯 Python wheel 不代表底层依赖已经跨平台验证。

交付说明 SDK 版本、wheel 路径、SHA256、已验证的平台和行为，以及未完成的验证。正式发布使用通过验收的同一 wheel 与校验文件；当前流程不发布 PyPI，也不在分析过程中自动更新 SDK。
