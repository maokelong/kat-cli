# KAT 公共能力（kat-sdk）

`kat-sdk` 是独立于 `kat-workflow` 的官方公共能力 wheel。它提供可复用的 Provider、Workflow、Python helper 和运行时 Guide；未安装或卸载 SDK 不影响框架及不依赖 SDK 的 PACK。

当前 SDK 版本为 `0.1.1`，最低框架基线为 `0.1.1rc13`。当前支持 CPython 3.14 的 Windows/Linux x86_64 KAT 部署，正式发布前须在两种平台验证同一个候选 wheel。

## 使用场景与示例

可以向 KAT 提出：“有哪些可复用的 Trace 数据接入能力？”或“我想验证 KAT Workflow 到公共函数的调用链，生成一条给小王的问候。”前者是能力浏览，后者可选择 `demo-greeting` 后执行。普通分析先展示相关候选，用户选定后才采纳知识或运行；本 PACK 不代表已经具备任意性能问题的分析算法。

## 输入、结果与限制

`demo-greeting` 只需要非空称呼，默认 `KAT`；返回一行 `message`，仅用于机制演示，不能形成性能结论。参数以 `kat inspect workflow --pack kat-sdk --workflow demo-greeting` 为准；结果解释见随 Run 保存的指南，当前说明可查[问候示例 Guide](knowledge/workflows/demo/greeting.md)。

Ftrace 和 Trace Streamer 是供程序作者使用的 Provider，不是直接运行的用户分析入口。Ftrace 需要文本 Trace、来源时钟名称和有效物化工作目录，见 [Ftrace 来源说明](knowledge/providers/ftrace.md)；Trace Streamer 接受已有 SQLite 或由外部工具生成的数据库，具体前提和限制见 [Trace Streamer 来源说明](knowledge/providers/trace-streamer-sqlite.md)。是否能回答用户问题取决于实际调用它们的 Workflow 和证据。

## 源码布局

```text
kat/sdk/
├─ providers/               # 可导入 Provider
├─ workflows/               # 由 PACK declaration 发现的 Workflow
├─ helpers/                 # 可导入公共函数
├─ knowledge/               # declaration 直接引用的 Runtime Guide
│  ├─ providers/
│  └─ workflows/
├─ docs/
│  ├─ api.md                # 简短 Python API 目录
│  └─ reference/            # 每个公共模块一份详细参考
└─ tests/
```

Provider 和 Workflow 的 Guide 由各自 declaration 的 `guide=` 路径定位；Provider Guide 通过 `kat inspect provider` 读取，Workflow Guide 在执行后由 Run 返回或 `kat inspect run` 读取；`knowledge/` 不维护导航首页。

## Python API 文档

[源码 API 文档](https://github.com/maokelong/kat-cli/blob/main/kat/sdk/docs/api.md)描述 SDK 公共 Python 模块并导航到逐模块 reference；程序作者使用与 SDK 配对、经审核的 `kat-author` 资料。源码文档不随 wheel 交付。`kat/sdk/__init__.py` 中私有、有序的 `_API_MODULES` 是模块清单；每个模块的字面量 `__all__` 是符号清单。Workflow 的公共身份来自 PACK declaration、inspection 和 Guide，不进入 Python API reference。

API 文档由 `kat-dev-sdk` 从当前源码 checkout 静态生成。生成时读取签名、类型注解、docstring、实现、测试和真实消费者，不 import 或执行 SDK 模块；示例只能来自已有测试或真实消费者。生成结果必须人工审核。审核后由用户自行把 `docs/api.md` 和整个 `docs/reference/` 复制到 `kat/skills/kat-author/references/`，`kat-dev-sdk` 不自动复制。

API 文档不进入 wheel。wheel 构建不生成、修改或复制 `docs/`，也不打包 `api.md`、`reference/` 或旧的 `*.api.md`。本 README 与 `knowledge/` 中 declaration 引用的 Runtime Guide 随 wheel 交付。

## 开发者复用

- `kat_sdk.providers.ftrace.FtraceProvider`：解码或复用 tracefs 文本，以 DataFusion SQL 查询。
- `kat_sdk.providers.trace_streamer.TraceStreamerProvider`：打开或生成 Trace Streamer SQLite，以只读 SQLite SQL 查询。
- `kat_sdk.helpers.demo.greeting.build_greeting`：生成中文问候语；`demo-greeting` Workflow 使用它演示 Workflow 到公共函数的最小链路。

具体 Python 调用以配对的作者 API 文档为入口；Provider 的来源、Schema 和查询语义以 inspection 返回的 Guide 为准；Workflow 使用 `kat inspect workflow --pack kat-sdk` 发现。

## 构建与验收

从仓库根目录执行：

```text
python build/build_sdk_wheel.py --output target/sdk-wheel
python build/verify_sdk_install.py --kat <当前源码构建的CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output target/sdk-verification
```

构建产出 wheel 与对应 SHA256。验收脚本在隔离部署中验证未安装、安装、升级和卸载，并覆盖发现、Runtime Guide、实际执行和查询。发布使用已验收的同一 wheel 与摘要；当前流程不发布 PyPI，也不自动创建 Release。

详细开发和构建流程见同套部署中的 `kat-dev-sdk`；源码设计见[公共 SDK 规格](https://github.com/maokelong/kat-cli/blob/main/docs/specs/public-capability-sdk.md)。

<!-- kat:capabilities:start -->
## 当前能力（自动同步）

**KAT 公共能力**（`kat-sdk`）；维护方：KAT。

由官方维护、供各领域复用的公共 Workflow。

### 知识入口

尚无已发布知识入口。

### Workflow

#### `demo-greeting`

调用 demo 公共库，生成一行问候结果。

输入合同：`kat inspect workflow --pack kat-sdk --workflow demo-greeting`。外部 PACK 继续携带 `--pack-dir <PACK绝对目录>`。

| 参数 | 类型 | 必填 | 默认值 | 可选值 | 含义 |
| --- | --- | --- | --- | --- | --- |
| --name | string | 否 | &quot;KAT&quot; | — | 称呼；首尾空白会被去除，不能全为空白。 |

Provider 与普通函数的复用说明见作者维护的正文；它们不是直接分析入口。
<!-- kat:capabilities:end -->
