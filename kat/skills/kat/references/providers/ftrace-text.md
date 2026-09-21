# FtraceProvider：tracefs 文本查询

适用于 SDK 0.1.1。把 tracefs 文本转换为可通过 DataFusion SQL 查询的类型化关系，适合在 Workflow 中读取线程调度、事件头和支持的事件 payload。

使用前读取当前版本的完整数据合同：

```text
kat inspect provider --provider ftrace-text
```

按 [命令合同](../command-reference.md) 定位 CLI；在 Workflow 中调用：

```python
from pathlib import Path
from kat_sdk.providers.ftrace import FtraceProvider

provider = FtraceProvider(
    source=Path(trace_path),
    clock_domain=clock_domain,
    workspace_root=ctx.datasource_root,
)
table = provider.query("SELECT * FROM text_ftrace_header")
```

`trace_path` 为输入文件路径，`clock_domain` 为调用方明确提供的非空采集时钟域。输出是框架 Table，在 Workflow 中返回后才成为 Run Output。使用 `provider.tables` 或 `SHOW TABLES` 确认实际关系，再查询具体事件。

来源物化目录按文件 stem 标识；同一 Session 中同 stem 会复用已有目录。时钟域不匹配或目录无效会失败；不同来源应使用不同 stem 或 Session。SDK 依赖框架及原生 `kat-datasource`，仅导入成功不代表来源已成功解析。字段、表关联与时钟语义按 inspection Guide 核对。

返回 [Provider 导航](index.md)。
