# `kat_sdk.providers.trace_streamer`

打开已有 Trace Streamer SQLite，或调用指定解析器生成 SQLite，并执行只读查询。

## `TraceStreamerProvider`

```python
TraceStreamerProvider(
    *,
    source: pathlib.Path | None = None,
    executable: pathlib.Path | None = None,
    workspace_root: pathlib.Path | None = None,
    sqlite_path: str | pathlib.Path | None = None,
) -> None
```

两种构造模式必须二选一：

- 已有数据库：只传 `sqlite_path`。路径必须是现有 SQLite 普通文件的精确绝对路径，数据库须通过完整性检查并包含至少一个关系。
- 解码来源：同时传入 `source`、`executable` 和 `workspace_root` 三个 `Path`。Provider 在 `workspace_root/<source stem>/trace.db` 复用或发布物化；SDK 不附带 Trace Streamer 可执行文件。

解码模式以调用方传入路径的词法 `source.stem` 作为 workspace 内的缓存键；解析后的来源位置不参与键计算。同一 workspace 中 stem 相同的路径共享目标。现有目标优先于源文件和解析器检查：目标存在时只验证并打开它，不启动外部进程。

目标不存在时，Provider 在 workspace 下的临时候选目录运行外部进程，并在数据库完整且包含关系后发布。失败候选会清理；并发发布冲突只接受完整赢家。已有坏目标会保留并原样报错，不清理、覆盖或重新解码。

| 异常 | 稳定触发条件 |
| --- | --- |
| `TypeError` | `sqlite_path` 不是字符串或 `Path`；解码模式任一参数不是 `Path`，包括缺少参数。 |
| `ValueError` | 两种模式混用；`sqlite_path` 不是绝对、精确的普通文件；workspace 或已有物化不是普通目录、其中 `trace.db` 不是普通文件；无缓存时 source 或 executable 不是现有文件；source stem 无效；数据库完整性失败或没有关系。 |
| `RuntimeError` | 外部解析器返回非零退出码。 |
| `sqlite3.DatabaseError` | SQLite 无法打开、校验或读取已有目标或候选数据库。 |
| `OSError` | 外部进程无法启动，或候选发布失败且没有可接受的并发赢家。 |

### `query`

```python
def query(
    self,
    sql: str,
    *,
    schema: pyarrow.Schema,
    params: collections.abc.Mapping[str, object] | None = None,
) -> kat.dataprovider.Table
```

执行一条只读 SQLite SQL，并按 `schema` 返回 eager `Table`。查询列名和顺序必须与 `schema` 完全一致；`params` 只接受字符串键的命名映射。逐行转换严格遵守字段的 Python 标量类型、数值范围和 nullability，空结果仍保留声明的完整 Schema 及 metadata。

- `sql`、`schema`、`params` 或行值的 Python 标量类型无效时抛出 `TypeError`。
- 查询列与 Schema 不一致，或行值溢出声明类型、违反 nullability 时抛出 `ValueError`。
- 非只读操作、无效 SQL 或数据库访问失败时抛出 `sqlite3.DatabaseError`。

表结构、内部 ID 与时间语义以 `kat inspect provider --provider trace-streamer-sqlite` 返回的 Guide 为准。

经行为测试验证的已有数据库用法：

```python
import pyarrow as pa
from kat_sdk.providers.trace_streamer import TraceStreamerProvider

provider = TraceStreamerProvider(sqlite_path=database_path)
table = provider.query(
    "SELECT value FROM event WHERE value >= :minimum ORDER BY value",
    schema=pa.schema([pa.field("value", pa.int64(), nullable=False)]),
    params={"minimum": 2},
)
```
