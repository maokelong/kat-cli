# `kat_sdk.providers.ftrace`

将 tracefs 文本解码或复用为可通过 DataFusion SQL 查询的类型化关系。

## `FtraceProvider`

```python
FtraceProvider(
    *,
    source: pathlib.Path,
    clock_domain: str,
    workspace_root: pathlib.Path,
) -> None
```

构造时以调用方传入路径的词法 `source.stem` 作为 `workspace_root` 内的缓存键；解析符号链接或绝对路径不会改变该键。同一 workspace 中来自不同目录但 stem 相同的来源会复用同一物化。

现有目标优先：`workspace_root/<source stem>` 一旦存在，Provider 直接打开它，不再检查源文件或重新解码。目标不存在时才要求 `source` 是文件并启动解码；解码报告“目标已存在”且并发赢家已经发布时，Provider 打开赢家。已有或新发布的坏物化会保留，打开时来自框架、Arrow/Parquet 或 DataFusion 的错误原样透传，不自动清理或重解码。

| 参数 | 说明 |
| --- | --- |
| `source` | tracefs 文本文件。已有有效物化时无需再次读取该文件。 |
| `clock_domain` | 调用方明确提供的采集时钟域。 |
| `workspace_root` | 来源物化根，通常传入 `ctx.datasource_root`。 |

| 异常 | 稳定触发条件 |
| --- | --- |
| `TypeError` | `source` 或 `workspace_root` 不是 `pathlib.Path`，`clock_domain` 不是字符串，或传入签名中不存在的生命周期参数。 |
| `ValueError` | `clock_domain.strip()` 后为空，或词法 source stem 为空、保留或不适合作为目录名。 |
| `RuntimeError` | workspace 不是已有目录、首次解码时源文件不存在、解码器失败且没有并发赢家、解码未产生普通目录，或缓存时钟域与请求不一致。 |

打开物化及创建查询 Provider 时的框架、Arrow/Parquet 和 DataFusion 异常不改写类型；坏物化会保留，后续构造仍优先打开并再次暴露该错误。

### `decode_report`

```python
@property
def decode_report(self) -> kat_datasource.text_ftrace.DecodeReport
```

返回解码报告，其中包含来源内合法但尚未支持的事件名。

### `tables`

```python
@property
def tables(self) -> tuple[str, ...]
```

返回当前物化实际包含的 relation 名称。

### `query`

```python
def query(
    self,
    sql: str,
    *,
    params: collections.abc.Mapping[str, object] | None = None,
) -> kat.dataprovider.Table
```

执行引用当前来源关系的 DataFusion SQL；`params` 为命名参数。返回 eager `Table`，零行结果仍保留 Schema。SQL、参数或 DataFusion 执行错误原样透传。关系、字段和时钟语义以 `kat inspect provider --provider ftrace-text` 返回的 Guide 为准。

经现有消费者验证的最小用法：

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
