# Data Provider reference PACK

这是 KAT Skill 随源码维护的唯一公共 reference PACK。它在同一个 `datasources/` 模块
下提供三个 sibling Provider，展示 PACK 作者如何用 `@kat.provider` 配合
`kat.dataprovider` 承接远端数据库、本地文本和本地二进制解析器：

| Provider | 来源与物化 | 查询方式 | 示例 Workflow |
|---|---|---|---|
| `PostgreSQLProvider` | ADBC 直接读取远端结果 | PostgreSQL SQL | `query-observations`、`fuse-observations` |
| `FtraceTextProvider` | Python 解析 tracefs 文本并写入两张 Parquet 表 | DataFusion SQL | `summarize-ftrace-events` |
| `TraceStreamerProvider` | Trace Streamer 将 Htrace 物化为临时 SQLite | SQLite SQL | `summarize-native-hook` |

Provider 都是 PACK 自有的普通 Python 类。`@kat.provider` 只附加 inspection 元数据；
KAT 可以发现声明，但不会构造或包装 Provider。Workflow 显式调用 `decode()`、`query()`
和 Data Provider Toolkit。只有 Workflow 返回的 `dp.Table` 会发布为 Run Output，
中间 Table 和临时物化数据不会自动成为 Output。

## 目录

```text
dataprovider-pack/
├─ pack.toml
├─ datasources/
│  ├─ postgresql.py
│  ├─ ftrace.py
│  └─ trace_streamer.py
├─ knowledge/
│  ├─ providers/
│  └─ workflows/
├─ workflows/
│  ├─ query_observations.py
│  ├─ fuse_observations.py
│  ├─ summarize_ftrace_events.py
│  └─ summarize_native_hook.py
└─ tests/
   ├─ postgresql/
   ├─ ftrace/
   └─ trace_streamer/
```

`dp.DataFusionProvider` 是 KAT Toolkit，不是这个 PACK 的第四个 Provider。本地 Parquet
也不需要再包装一层 Provider；融合 Workflow 直接通过 `dp.open(tables=...)` 打开明确的
relation。

## PostgreSQL：直接查询与本地融合

`PostgreSQLProvider.query()` 每次按 libpq service 和显式 Database 创建独立连接，
在服务端开启只读事务，通过 ADBC 绑定位置参数并完整读取结果。Provider 返回 eager
`dp.Table` 前关闭 reader、cursor 和 connection，并回滚事务；后续读取、融合和 Output
发布不会再次执行远端 SQL。

ADBC 结果在进入 `dp.Table.from_arrow()` 前遵守以下边界：

- PostgreSQL `NUMERIC` 必须能够无舍入地表示为 `decimal128(38, 18)`；
- 有绝对时间语义的 timestamp 规范为 `timestamp(ns, tz="UTC")`；
- `TIMESTAMP WITHOUT TIME ZONE` 必须由来源 SQL 按领域规则显式转换，否则失败。

`query-observations` 直接发布一次远端查询。`fuse-observations` 依次查询 telemetry、
control 两个 Database，再把两个内存 Table 与本地 `thread_placement.parquet` 组合：

```python
from kat import dataprovider as dp

placement = dp.open(
    tables={"thread_placement": placement_root / "thread_placement.parquet"}
)
result = dp.DataFusionProvider(
    tables={"telemetry": telemetry, "processes": processes},
    catalog=placement,
).query("SELECT ...")
```

融合只按业务键执行；示例不在缺少共同 Clock domain 证据时比较不同来源的裸时间整数。
连接 URI、用户和凭据不作为 Workflow 参数，源码也不读取或记录 service/password file
内容。

## Ftrace：文本解析、Parquet 与 DataFusion

`FtraceTextProvider.decode()` 使用 Python 单遍按行解析 tracefs 文本：

```text
tracefs text
  -> FTRACE_SCHEMA.create() 创建 capture、events 两张可追加 Table
  -> append() Python 标量
  -> dp.write(tables) 一次写入两张 Parquet 表
  -> dp.open(tables=...) 打开完整 Catalog
  -> dp.DataFusionProvider(catalog=...) 查询
```

调用方显式提供 `clock_domain`。`clock_value` 是该 domain 下每秒 10 亿 tick 的原生
读数，不是 Unix epoch；转换使用十进制字符串，不经过浮点数。解析错误报告来源行号，
任何失败都使 Provider 保持未 ready 并尽力删除其独占 catalog。成功 decode 后可反复
query，返回的 eager Table 不再依赖临时 Parquet。

## Trace Streamer：二进制解析与 SQLite

`TraceStreamerProvider.decode()` 在 Workflow 独占的临时目录中用参数列表启动部署时
批准的 Trace Streamer，不使用 shell。只有进程成功、输出是普通 SQLite 文件、
`quick_check` 通过且存在业务 relation 时才进入 ready 状态。

`query()` 用 `mode=ro`、`PRAGMA query_only` 和 authorizer 限制为只读 SQLite 查询。
调用方用基础 Python 类型声明结果列，列名和顺序必须与 SQL 结果完全一致；Provider
关闭 cursor 与 connection 后再返回 eager `dp.Table`。生产 Workflow 从
`KAT_TRACE_STREAMER_EXECUTABLE` 读取批准的可执行文件路径，不把可执行代码路径暴露为
Workflow argument。

## 检查与默认测试

import 阶段不会连接数据库、读取 trace 或检查外部 executable，因此没有外部环境也可
分别列出全部 Workflow 和 Provider。以下路径以组装后的 Skill 根为准；实际调用时仍按
公共命令速查选择平台 `kat` 载荷并使用绝对路径：

```bash
kat inspect workflow \
  --pack dataprovider-pack \
  --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack
kat inspect provider \
  --pack dataprovider-pack \
  --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack

kat test --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack
```

默认测试只收集 `test_*.py`：fake ADBC 合同、小型 Ftrace fixture，以及模拟 Trace
Streamer executable/SQLite。它们不访问真实 PostgreSQL、真实大 trace 或真实二进制。

## 显式真实测试

真实文件和数据库不提交到仓库，`real_*.py` 也不被默认 pytest 规则收集。显式执行时
缺少前置条件会失败，不会 skip。

PostgreSQL 测试需要同一 service 下的两个不同 Database，以及只读和 writer 两个
fixture profile：

```bash
export PGSERVICEFILE=/absolute/path/to/pg_service.conf
export PGPASSFILE=/absolute/path/to/pgpass
export KAT_TEST_POSTGRES_READONLY_PROFILE=readonly_service
export KAT_TEST_POSTGRES_WRITER_PROFILE=writer_fixture_service
export KAT_TEST_POSTGRES_TELEMETRY_DATABASE=telemetry
export KAT_TEST_POSTGRES_CONTROL_DATABASE=control

kat test --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack \
  --test tests/postgresql/real_postgresql.py
kat test --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack \
  --test tests/postgresql/real_fusion.py
```

Ftrace 真实合同：

```bash
export KAT_TEST_FTRACE_PATH=/absolute/path/to/kat_complex_20260818.ftrace
kat test --pack-dir /absolute/path/to/kat-skill/references/examples/dataprovider-pack \
  --test tests/ftrace/real_ftrace.py
```

Trace Streamer 真实合同：

```powershell
$env:KAT_TEST_TRACE_STREAMER_EXE = "C:\absolute\path\to\trace_streamer.exe"
$env:KAT_TEST_HTRACE_PATH = "C:\absolute\path\to\trace.htrace"
kat test `
  --pack-dir "C:\absolute\path\to\kat-skill\references\examples\dataprovider-pack" `
  --test tests/trace_streamer/real_trace_streamer.py
```
