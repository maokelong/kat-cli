# SDK 公共库

公共函数为 Workflow 和 Provider 提供可复用的实现。开发两者或在 Python 中直接调用 SDK 公共函数时，从这里选择领域和方法。SDK 开发使用同级 `kat-dev-sdk`；本目录介绍已有公共库。

- demo：[问候语 build_greeting](demo/greeting.md)，适用于 SDK 0.1.1 起的示例调用。

公共库直接从 `kat_sdk.helpers.<领域>.<模块>` 导入，不注册为 CLI 能力。先按 [Python 依赖管理](../python-packages.md) 确认当前部署的 SDK 版本；SDK 未安装时，安装或升级按用户指令执行。

公共库的使用说明和 API Markdown 全部随 KAT Skills 交付，不在 SDK 的 knowledge 目录中保留副本。每篇文档应写明适用 SDK 版本、导入路径、签名、参数、返回值、异常与示例。SDK 独立升级后，核对安装版本；必要时用 Python 的 `inspect.signature()` 和 `inspect.getdoc()` 查看已安装函数。

新增公共库时，在本目录按领域添加文档并更新导航，同时维护 SDK 中对应的类型注解和 docstring。

开发领域 PACK 公共函数见 [kat-author](../../../kat-author/SKILL.md)，开发官方 SDK 公共函数见 [kat-dev-sdk](../../../kat-dev-sdk/SKILL.md)。
