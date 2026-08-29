# Ftrace Text Provider example PACK

这个 External PACK 展示用户如何在 PACK 的 `datasources/` 中用普通 Python 类解析
本地文本文件。`FtraceTextProvider` 不继承 KAT 类型，也不需要注册；Workflow 显式
调用 `decode()` 和 `query()`：

```text
tracefs text
  -> Python 单遍按行解析
  -> ds.write(FTRACE_SCHEMA) 分批写入两张 Parquet 表
  -> ds.open() 打开可重复查询的 Catalog
  -> Provider.query(SQL) 返回 eager ds.Table
  -> Workflow 直接发布 main Output
```

## 数据合同

Provider 使用基础 Python 类型声明两张表：

| 表 | 列 |
|---|---|
| `capture` | `tracer: str`、`entries_in_buffer: int`、`entries_written: int`、`cpu_count: int` |
| `events` | `timestamp_ns: int`、`cpu: int`、`comm: str`、`pid: int`、`tgid: int \| None`、`flags: str`、`event: str`、`details: str` |

`timestamp_ns` 是 tracefs 当前 trace clock 的整数读数，不是 Unix epoch，也不是 UTC
时间。Provider 用十进制字符串直接换算纳秒，不经过浮点数。没有 TGID 的事件写入
`None`；无法识别的非注释行立即失败，并报告原文件行号。

## Provider 使用方式

```python
from pathlib import Path

from kat.pack.datasources.ftrace import FtraceTextProvider


provider = FtraceTextProvider(
    source=Path(trace_path),
    materialization_root=ctx.datasource_root / "ftrace-text",
).decode()

events = provider.query(
    """
    SELECT event, COUNT(*) AS event_count
    FROM events
    GROUP BY event
    ORDER BY event_count DESC, event
    """
)
```

`decode()` 每次只重建 `materialization_root/catalog`；不会删除同一根目录下由
Workflow 或其他 Provider 拥有的文件。解析按 4096 行分 batch 写入，不把整个输入
文件读进内存。成功后同一个 Provider 可以反复查询；解析失败时 Provider 保持未
ready，修正输入后可以再次调用 `decode()`。本例不把旧 catalog 当作跨进程状态，
不提供缓存、迁移或恢复协议。

`query()` 委托 KAT 的只读本地 Catalog，因此支持具名 `params`，并在返回前形成与
查询 Session 脱离的 eager `ds.Table`。PACK 作者可以直接返回这个 Table，也可以把
它作为 `ctx.sql(tables=...)` 的显式多源融合输入。

## 运行 Workflow

仓库内的 `summarize-ftrace-events` Workflow 从 `ctx.datasource_root` 派生 Provider
目录，解析输入并直接返回按事件名称统计的 Table：

```bash
kat inspect --pack ftrace-text-provider \
  --pack-dir ./examples/packs/ftrace-text-provider

kat run \
  --pack ftrace-text-provider \
  --workflow summarize-ftrace-events \
  --pack-dir ./examples/packs/ftrace-text-provider \
  -- \
  --trace-path /absolute/path/to/trace.ftrace

kat query \
  --run <run-id> \
  --sql "SELECT * FROM output.main ORDER BY event_count DESC, event"
```

## 测试

默认测试使用仓库内的小型文本 fixture，覆盖多表解析、纳秒精度、TGID 空值、分批
写入、坏行诊断、失败重试、重复 decode/query 和 Workflow Output：

```bash
kat test --pack-dir ./examples/packs/ftrace-text-provider
```

真实大样本不提交到仓库，也不被默认 `kat test` 收集。显式选择测试文件并通过
`KAT_TEST_FTRACE_PATH` 提供样本；变量缺失或路径不可读会失败，不会 skip：

```bash
export KAT_TEST_FTRACE_PATH=/absolute/path/to/kat_complex_20260818.ftrace
kat test \
  --pack-dir ./examples/packs/ftrace-text-provider \
  --test tests/real_ftrace.py
```

真实合同验证 `nop` header、44,344 条事件、4 个 CPU，以及第一条事件的全部字段。
