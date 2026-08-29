# Trace Streamer SQLite Provider example PACK

这个 External PACK 展示用户如何在 PACK 的 `datasources/` 中包装一个已有本地二进制
解析器。`TraceStreamerProvider` 是 PACK 自己拥有的普通 Python 类，不继承 KAT 类型、
不注册，也不把 Trace Streamer 生成的 SQLite 整体转换为 Parquet：

```text
.htrace
  -> TraceStreamerProvider.decode()
  -> trace_streamer <source> -e <fresh workspace>/trace.db
  -> SQLite quick_check + relation validation
  -> TraceStreamerProvider.query(SQL, schema=...)
  -> eager ds.Table
  -> Workflow 直接发布 main Output
```

## Provider 使用方式

Workflow 显式提供来源、解析器与当前 PACK 在 KAT Data Home 下的物化根目录：

```python
from pathlib import Path

from kat.pack.datasources import trace_streamer


provider = trace_streamer.TraceStreamerProvider(
    source=Path(htrace_path),
    executable=Path(trace_streamer_path),
    materialization_root=ctx.datasource_root / "trace-streamer",
).decode()

summary = provider.query(
    trace_streamer.NATIVE_HOOK_SUMMARY_SQL,
    schema=trace_streamer.NATIVE_HOOK_SUMMARY_SCHEMA,
)
```

`decode()` 每次都重建 `materialization_root/workspace`；同一物化根即使由新的
Provider 实例接管，也不会把旧 DB 当成本次结果，同时不会删除根目录下的其他文件。
它以参数列表
`[executable, source, "-e", database]`、`shell=False` 和该目录作为 `cwd` 启动
Trace Streamer。只有本次进程退出码为 0、输出是普通文件、SQLite
`quick_check` 成功且至少存在一张业务表或视图时，Provider 才进入 ready 状态。
任何失败都会清理本次 workspace；旧 DB 不会被当成本次成功。

`query()` 使用标准库 `sqlite3` 以 `mode=ro` 打开 DB，并额外启用
`PRAGMA query_only`。SQL 方言、参数绑定和来源内 Join/Aggregate 均属于 SQLite；
Provider 会完整 `fetchall()`，关闭 cursor 与 connection，再用显式 Python Schema
构造可重复读取的 `ds.Table`。因此空结果和全 NULL 结果也有确定类型，返回 Table
不再依赖 SQLite 连接，可以直接作为 Run Output 或交给 `ctx.sql(tables=...)` 融合。

查询列名及顺序必须与 Schema 完全一致。Schema 只使用 KAT Datasource 支持的 Python
类型，例如：

```python
NATIVE_HOOK_SUMMARY_SCHEMA = {
    "event_type": str,
    "event_count": int,
    "total_heap_size": int,
}
```

## 运行 Workflow

`summarize-native-hook` 解析一份 HiTrace，并直接返回按 `event_type` 聚合的
`native_hook` 结果：

```bash
kat inspect --pack trace-streamer-sqlite-provider \
  --pack-dir ./examples/packs/trace-streamer-sqlite-provider

kat run \
  --pack trace-streamer-sqlite-provider \
  --workflow summarize-native-hook \
  --pack-dir ./examples/packs/trace-streamer-sqlite-provider \
  -- \
  --source-path /absolute/path/to/trace.htrace \
  --trace-streamer-path /absolute/path/to/trace_streamer

kat query \
  --run <run-id> \
  --sql "SELECT * FROM output.main ORDER BY event_type"
```

## 测试

默认测试使用 Python 自身模拟外部解析器并生成临时 SQLite，覆盖 fresh workspace、
失败清理、DB 校验、只读查询、显式 Schema、参数绑定、重复查询与 Workflow Output，
不依赖真实 Trace Streamer 或大 trace：

```bash
kat test --pack-dir ./examples/packs/trace-streamer-sqlite-provider
```

真实二进制和 `.htrace` 不提交到仓库，也不被默认 `kat test` 收集。显式选择
`tests/real_trace_streamer.py`，并提供两个环境变量；变量缺失或路径非法会失败，不会
skip：

```powershell
$env:KAT_TEST_TRACE_STREAMER_EXE = `
  "D:\work\kat_rs\0812\kat-cli\test\trace_streamer\windows\release\trace_streamer.exe"
$env:KAT_TEST_HTRACE_PATH = `
  "D:\work\kat_rs\0812\kat-cli\test\all_memory_full.htrace"

kat test `
  --pack-dir ./examples/packs/trace-streamer-sqlite-provider `
  --test tests/real_trace_streamer.py
```

真实合同验证四行聚合：`AllocEvent` 为 `114976 / 21964373`、`FreeEvent` 为
`110359 / 20577720`、`MmapEvent` 为 `64 / 11538432`、`MunmapEvent` 为
`57 / 3014656`（事件数 / `heap_size` 总和）。
