# TraceStreamerProvider：Trace Streamer SQLite 查询

适用于 SDK 0.1.1。用于查询 Trace Streamer 已生成的 SQLite，或调用显式指定的 Trace Streamer 程序解析来源。SDK 不附带外部 Trace Streamer 可执行文件。

按 [命令合同](../command-reference.md) 定位 CLI，先读当前数据库语义与 SQL Guide：

```text
kat inspect provider --provider trace-streamer-sqlite
```

已有数据库模式：

```python
import pyarrow as pa
from kat_sdk.providers.trace_streamer import TraceStreamerProvider

provider = TraceStreamerProvider(sqlite_path=database_path)
table = provider.query(
    "SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name",
    schema=pa.schema([("name", pa.string())]),
)
```

`database_path` 必须是指向现有有效 SQLite 文件的精确绝对路径。解析模式改用 `source=Path(trace_path)`、`executable=Path(tool_path)`、`workspace_root=ctx.datasource_root`，三者均需提供；不能同时传 `sqlite_path`。

`query(sql, *, schema, params=None)` 执行单条只读 SQLite 查询，显式提供结果 Arrow Schema，返回框架 Table。先检查 `sqlite_schema` 中的实际表和列，再依据 Guide 关联；内部 ID 与系统 pid/tid、时间单位和时钟域需要区分。参数组合、路径或数据库无效会失败，外部解析器失败也会报错。

返回 [Provider 导航](index.md)。
