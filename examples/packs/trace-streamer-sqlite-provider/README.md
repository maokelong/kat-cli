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

Workflow 在 `ctx.datasource_root` 下创建本次调用独占的临时目录，再把来源、PACK
部署时批准的解析器和该目录显式交给 Provider。生产 Workflow 从
`KAT_TRACE_STREAMER_EXECUTABLE` 读取解析器位置，不把可执行代码路径开放为 Workflow
argument：

```python
import os
from pathlib import Path
from tempfile import TemporaryDirectory

from kat.pack.datasources import trace_streamer


with TemporaryDirectory(dir=ctx.datasource_root) as workspace:
    provider = trace_streamer.TraceStreamerProvider(
        source=Path(htrace_path),
        executable=Path(os.environ["KAT_TRACE_STREAMER_EXECUTABLE"]),
        workspace=Path(workspace) / "trace-streamer",
    ).decode()

    summary = provider.query(
        trace_streamer.NATIVE_HOOK_SUMMARY_SQL,
        schema=trace_streamer.NATIVE_HOOK_SUMMARY_SCHEMA,
    )
```

Workflow 拥有临时目录的生命周期，并从中派生尚不存在的 `trace-streamer` 子目录；
Provider 把该子目录视为独占目标。`decode()`
每次先清理并重建该目录，不会把旧 DB 当成本次结果。它以参数列表
`[executable, source, "-e", database]`、`shell=False` 和该目录作为 `cwd` 启动
Trace Streamer。只有本次进程退出码为 0、输出是普通文件、SQLite
`quick_check` 成功且至少存在一张业务表或视图时，Provider 才进入 ready 状态。
任何失败都会保持 Provider 未准备并尽力清理本次 workspace；成功查询得到 eager
Table 后，Workflow 即可退出 `with` 并删除 SQLite。

`query()` 使用标准库 `sqlite3` 以 `mode=ro` 打开 DB，并额外启用
`PRAGMA query_only`。连接 authorizer 只允许形成只读查询的 SELECT、READ、FUNCTION 和
RECURSIVE 动作，拒绝 `ATTACH`、`DETACH`、PRAGMA、DDL、DML 与事务修改，避免查询在
目标 DB 之外创建文件。查询表达式、参数绑定和来源内 Join/Aggregate 仍使用 SQLite
语义；Provider 用显式 Python Schema 创建 `ds.Table`，逐行 `append()` 查询结果，
并在返回前关闭 cursor 与 connection。因此空结果和全 NULL 结果也有确定类型，返回
Table 不再依赖 SQLite 连接，可以直接作为 Run Output；需要多源查询时再显式交给
`ds.DataFusionProvider(tables=...)`。

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

export KAT_TRACE_STREAMER_EXECUTABLE=/absolute/path/to/trace_streamer

kat run \
  --pack trace-streamer-sqlite-provider \
  --workflow summarize-native-hook \
  --pack-dir ./examples/packs/trace-streamer-sqlite-provider \
  -- \
  --source-path /absolute/path/to/trace.htrace

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

真实合同先确认解析器报告精确版本 `4.3.7`（该版本的 `--version` 历史退出码为
`1`），再验证四行聚合：`AllocEvent` 为
`114976 / 21964373`、`FreeEvent` 为
`110359 / 20577720`、`MmapEvent` 为 `64 / 11538432`、`MunmapEvent` 为
`57 / 3014656`（事件数 / `heap_size` 总和）。
