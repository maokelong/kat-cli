# Trace Streamer SQLite Provider 作者知识

`TraceStreamerProvider(source, executable, workspace)` 展示如何接入已有本地二进制解析器。
`decode()` 以参数数组运行批准的 Trace Streamer：

```text
Htrace 文件 -> trace_streamer <source> -e <workspace>/trace.db -> SQLite
```

调用不经过 shell。Provider 只在进程成功、输出是普通文件、SQLite `quick_check` 通过且
至少存在一张业务 table/view 后进入 ready 状态；失败会清理自己独占的 workspace。
可执行文件路径属于部署配置，本示例 Workflow 从 `KAT_TRACE_STREAMER_EXECUTABLE` 读取，
不把它当作可能进入日志的业务参数。

`query(sql, schema=..., params=...)` 直接在生成的 SQLite 上执行来源内 SQL，并在连接
关闭后从完整 `pyarrow.Table` 构造不可变 eager `dp.Table`。连接使用 `mode=ro`、
`PRAGMA query_only` 和 SQLite authorizer，仅允许只读查询。调用方必须用基础 Python 类型
按 SQL 投影顺序声明结果 Schema；实际列名或顺序不一致会失败。

示例只依赖 Trace Streamer 的 `native_hook` relation，并查询：

- `event_type`：事件类别；
- `heap_size`：该事件贡献的堆大小。

示例结果按 `event_type` 分组，产生 `event_type: str`、`event_count: int`、
`total_heap_size: int`。Trace Streamer 版本可能提供更多表；新增 Workflow 前应以目标
版本生成的 SQLite schema 为事实，先写明确 SQL 与输出 Schema，再决定哪些表需要关联。
同一 SQLite 内的 JOIN 和聚合应留在 Provider SQL 中；只有跨来源融合才把查询结果
Table 交给 DataFusion。
