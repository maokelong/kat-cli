# kat-sdk

`kat-sdk` 是独立于 `kat-workflow` 的官方公共能力 wheel。它提供可复用的 Provider、Workflow、Python helper 和运行时 Guide；未安装或卸载 SDK 不影响框架及不依赖 SDK 的 PACK。

当前 SDK 版本为 `0.1.1`，最低框架基线为 `0.1.1rc13`。当前支持 CPython 3.14 的 Windows/Linux x86_64 KAT 部署，正式发布前须在两种平台验证同一个候选 wheel。

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

Provider 和 Workflow 的 Guide 由各自 declaration 的 `guide=` 路径定位，通过 `kat inspect provider` 或 `kat inspect workflow` 读取；`knowledge/` 不维护导航首页。

## Python API 文档

[`docs/api.md`](docs/api.md) 描述 SDK `0.1.1` 的公共 Python 模块，并按需链接到 [`docs/reference/`](docs/reference/)。`kat/sdk/__init__.py` 中私有、有序的 `_API_MODULES` 是模块清单；每个模块的字面量 `__all__` 是符号清单。Workflow 的公共身份来自 PACK declaration、inspection 和 Guide，不进入 Python API reference。

API 文档由 `kat-dev-sdk` 从当前源码 checkout 静态生成。生成时读取签名、类型注解、docstring、实现、测试和真实消费者，不 import 或执行 SDK 模块；示例只能来自已有测试或真实消费者。生成结果必须人工审核。审核后由用户自行把 `docs/api.md` 和整个 `docs/reference/` 复制到 `kat/skills/kat-author/references/`，`kat-dev-sdk` 不自动复制。

API 文档不进入 wheel。wheel 构建不生成、修改或复制 `docs/`，也不打包 `api.md`、`reference/` 或旧的 `*.api.md`。`knowledge/` 中 declaration 引用的 Runtime Guide 继续随 wheel 交付。

## 当前能力

- `kat_sdk.providers.ftrace.FtraceProvider`：解码或复用 tracefs 文本，以 DataFusion SQL 查询。
- `kat_sdk.providers.trace_streamer.TraceStreamerProvider`：打开或生成 Trace Streamer SQLite，以只读 SQLite SQL 查询。
- `kat_sdk.helpers.demo.greeting.build_greeting`：生成中文问候语；`demo-greeting` Workflow 使用它演示 Workflow 到公共函数的最小链路。

具体 Python 调用以 [`docs/api.md`](docs/api.md) 为入口；Provider 的来源、Schema 和查询语义以 inspection 返回的 Guide 为准；Workflow 使用 `kat inspect workflow --pack kat-sdk` 发现。

## 构建与验收

从仓库根目录执行：

```text
python build/build_sdk_wheel.py --output target/sdk-wheel
python build/verify_sdk_install.py --kat <当前源码构建的CLI> --workflow-wheel <框架wheel> --datasource-wheel <当前平台原生wheel> --sdk-wheel <SDKwheel> --output target/sdk-verification
```

构建产出 wheel 与对应 SHA256。验收脚本在隔离部署中验证未安装、安装、升级和卸载，并覆盖发现、Runtime Guide、实际执行和查询。发布使用已验收的同一 wheel 与摘要；当前流程不发布 PyPI，也不自动创建 Release。

详细开发和构建流程见 [`kat-dev-sdk`](../skills/kat-dev-sdk/SKILL.md)，设计合同见 [`docs/specs/public-capability-sdk.md`](../../docs/specs/public-capability-sdk.md)。
