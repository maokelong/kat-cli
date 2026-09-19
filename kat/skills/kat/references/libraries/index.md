# SDK 公共库

需要在 Python 或 Workflow 中直接调用 SDK 公共函数时，从这里选择领域和方法。SDK 开发使用同级 `dev-sdk`；本目录介绍如何使用已有公共库。

- demo：[问候语 build_greeting](demo/greeting.md)，适用于 SDK 0.1.1 起的示例调用。

公共库直接从 `kat_sdk.libraries.<领域>.<模块>` 导入，不注册为 CLI 能力。先按 [Python 依赖管理](../python-packages.md) 确认当前部署的 SDK 版本；SDK 未安装时，安装或升级按用户指令执行。

介绍文档随 KAT Skills 交付。SDK 可以独立升级，因此函数的当前签名、异常和示例应同时核对已安装 SDK 的生成 API 参考：

```text
<bundled-python> -I -B -c "from importlib.resources import files; print(files('kat_sdk').joinpath('knowledge/index.md'))"
```

上面的路径是 SDK 当前知识首页，包含生成 API 链接。新增公共库时，在本目录按领域添加介绍文档并更新导航，在 SDK 中维护对应类型注解和 docstring。
