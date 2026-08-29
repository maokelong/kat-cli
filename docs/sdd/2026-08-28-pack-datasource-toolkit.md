---
status: accepted
---

# PACK Datasource Toolkit 简化设计

## 1. 背景

PR #228 把来源扩展拆成 PACK `SourceExecutor`、`ctx.provider()` 与 KAT `Provider` facade。虽然这条链路集中处理了来源结果落盘和注册，但同一个 Provider 概念同时指向 PACK 来源实现与 KAT facade，PACK 作者还需要理解 executor、facade、Arrow stream、scratch 和 Runtime 生命周期，公开模型超过了来源接入本身所需的复杂度。

本文取代 `2026-08-26-pack-datasource.md` 与 ADR-0062 中的 Provider 所有权和作者接口决定，并通过 ADR-0063、ADR-0065、ADR-0066 记录新的稳定边界。旧设计中仍然有效的多数据源本地融合、Run Output 和迁移期兼容原则在本文中重新表述。

## 2. 目标

- 一个 PACK 可以在普通 Python Datasource module 中定义多个来源 Provider；
- Provider 是 PACK 拥有并直接暴露给 Workflow 的普通类；
- Provider 可以按来源语义显式提供 `decode()`、`query()`、`materialize()` 等方法；
- 用户自定义解析算法，KAT 不建立统一 Parser 基类或 Parser registry；
- 自定义 Python Parser 可以按 Schema 写入 Python 列表，已有 binary Parser 也可以继续产生 Parquet 表或来源自己的本地数据库；
- KAT 提供低门槛的 Schema 定义、校验、多表 Parquet 落盘和本地查询 Toolkit；
- 继续支持多个来源结果在本地 DataFusion 中联合查询；
- 不依赖隐式当前 Context，不建立 Provider facade、Binding 或平台注册机制。

## 3. 设计全貌

### 3.1 一条标准数据链

```text
PACK Workflow
  ├─ 显式 import、构造并调用 PACK Provider
  │
  ├─ 文件来源
  │    ├─ Python Parser → ds.write() → Parquet files ┐
  │    └─ 已有 Parquet ──────────────────────────────┴→ ds.open() → Catalog
  │
  ├─ 数据库或远端服务
  │    └─ Provider 原生 query → Python rows 或 Arrow → ds.Table
  │
  └─ ds.Table / Parquet relation
       ├─ 由 Python 直接读取
       ├─ 直接作为单源 Workflow Output
       └─ 显式交给 ds.DataFusionProvider 做本地融合
                                      └─ ds.Table
                                           └─ Workflow 返回后由 Runtime 发布
```

`kat.pack.datasources.*` 保存 PACK 拥有的 Provider、Schema 与解析代码；`kat.datasource` 是 KAT 提供的普通数据面 Toolkit。KAT 不发现、注册、构造或包装 Provider。

Datasource Provider 负责数据从哪里来、如何解析、如何执行来源内查询，以及如何准备来源 backend；其方法集合、SQL 方言和参数完全属于来源。首版文件 backend 只服务当前 Workflow，不作为跨 Workflow cache。KAT 提供的具体 `ds.DataFusionProvider` 只组合 Workflow 显式提供的本地 Table 与 Parquet relation，不发现来源 Provider，也不隐式执行来源查询。`ctx.sql(sql, **params) -> DataFrame` 原样保留为迁移期旧 Dataset grant 的惰性兼容入口。

文件 Provider 可以把多表解析结果写入 `ctx.datasource_root` 下当前 Workflow 的临时 workspace，再用 `ds.open()` 建立只读 Catalog；查询 eager 返回脱离来源的 Table 后即可清理 workspace。数据库 Provider 不需要整体转储数据库，只把某次查询结果通过 `ds.Table(...)` 逐行追加，或通过 `ds.Table.from_arrow()` 形成 eager `ds.Table`。Workflow 返回后，Runtime 才把被选中的 Table 写成最终 Run Output；仅用于 Python 计算或融合的中间 Table 不产生 Output 文件。

### 3.2 对象关系

| 对象 | 所有者 | 负责 | 不负责 |
|---|---|---|---|
| Datasource Provider | PACK | 来源定位、解析、来源 SQL、来源资源和当前 Workflow backend | KAT 注册、跨源 planning、Run Output 发布、跨 Workflow cache |
| `ds.Schema` | PACK 声明、KAT 校验 | 多表逻辑结果合同 | Parser 算法、Schema 推断、演进 |
| `ds.write()` | KAT Toolkit | Table Mapping 的同步一表一文件 Parquet 写入 | artifact key、缓存、查询 |
| `ds.Catalog` | KAT Toolkit | 发现并校验具名 Parquet relation 与稳定路径 | Session、SQL、文件所有权、快照 |
| `ds.Table` | KAT Toolkit | eager、可重复读取的标准单表结果 | relation name、Provider identity、DataFrame API |
| `ds.DataFusionProvider` | KAT Toolkit | 显式内存 Table、Parquet 与混合本地查询 | 来源发现、远端下推、落盘、Output 发布 |
| Workflow Context | KAT Runtime | PACK 存储根、迁移期 Dataset grant 查询 | 新 Datasource 融合、Provider factory、Provider 生命周期、隐式 catalog |
| Workflow | PACK | 选择 Provider、调用顺序、融合步骤和最终 Output | 透明 federation、Runtime 发布实现 |

同一个 Workflow 可以顺序查询同一远端服务中的多个 Database。是否复用连接池、如何切库、SQL 参数和只读事务都由该 PACK 的 Provider 定义；KAT 只在各次查询已经形成 Table 后参与本地融合。

## 4. 公共 API 总览

以下签名描述首版规范表面。路径只接受 `pathlib.Path`；输入 Mapping 在调用开始时快照。`PythonColumnType` 表示 `bool`、`int`、`float`、`str`、`bytes`、UTC-aware `datetime.datetime` 或 `decimal.Decimal`，也可以写成 `T | None`。

```python
from kat import datasource as ds
```

### 4.1 Schema、Table 与 Parquet 写入

```python
schema = ds.Schema(
    tables: Mapping[str, Mapping[str, PythonColumnType]],
)

schema.tables -> tuple[str, ...]
schema[table_name] -> Mapping[str, PythonColumnType]

tables = schema.create() -> dict[str, ds.Table]

tables["events"].append(...)

ds.write(
    tables: Mapping[str, ds.Table],
    *,
    destination: pathlib.Path,
) -> None
```

Schema 至少包含一张表，每张表至少一列；声明在构造时复制并冻结。`schema.create()` 为每张声明产生一个可追加 Table。`ds.write()` 同步取得 Mapping 中各 Table 的调用时快照，要求 destination 不存在，并固定产生平铺的 `destination/<mapping-key>.parquet`；成功返回后需要另行调用 `ds.open()`。

### 4.2 Catalog

```python
catalog = ds.open(
    *,
    root: pathlib.Path | None = None,
    tables: Mapping[str, pathlib.Path] | None = None,
) -> ds.Catalog

catalog.tables -> tuple[str, ...]
```

`root` 与 `tables` 必须且只能提供一个。`ds.open()` 不接收 Datasource Schema，而是从 Parquet footer 取得每张 relation 的物理 Schema；Catalog 是经过路径、footer、锁定 DataFusion 扫描类型和分片一致性校验的具名 Parquet relation 集合及源路径只读 live view。Catalog 列可以宽于标准 Table 的 D41 集合。`catalog.tables` 只返回按 relation name 排序的名称 tuple；Catalog 不公开列 Schema API。它不创建 Session、不提供 `query()`、不构造 Table，也不预读数据行，只能显式传给 `ds.DataFusionProvider`。

### 4.3 Table 构造与读取

```python
table = ds.Table(schema: Mapping[str, PythonColumnType])
table.append(**row_values) -> None

table = ds.Table.from_arrow(arrow_table: pyarrow.Table) -> ds.Table
arrow_table = table.to_arrow() -> pyarrow.Table

len(table) -> int
table.columns -> tuple[str, ...]
table[column_name] -> tuple[object | None, ...]
table.to_rows() -> list[dict[str, object | None]]
```

普通 Python 路径先用单表列 Mapping 创建空 Table，再通过 `append()` 原子提交完整的一行；Python 值使用精确类型校验，不做跨类型推断。Arrow bridge 只准入可被锁定版本 PyArrow、DataFusion 与 Parquet 共同处理的扁平 Table 类型；两端共享只读 buffer，不深拷贝整表。`Table.from_arrow()` 与 DataFusion Provider 查询结果同样可以 `append()`，其已经准入的 Arrow 物理 Schema 就是追加合同。Output Query 的 JSON 结果类型是 D41 中单独定义的更窄集合。

Table 至少包含一列，列名必须是非空且唯一的字符串。Table 只允许按既有列结构追加完整行，不提供列结构 mutation、filter、select、group-by、表达式或 DataFrame API。

### 4.4 DataFusion Provider 与 Output

```python
fusion = ds.DataFusionProvider(
    *,
    tables: Mapping[str, ds.Table] | None = None,
    catalog: ds.Catalog | None = None,
)

result = fusion.query(
    sql: str,
    *,
    params: Mapping[str, Scalar] | None = None,
) -> ds.Table
```

`tables` 与 `catalog` 至少提供一个。Catalog relation name 来自 `ds.open(tables=...)` 的 Mapping key 或 `ds.open(root=...)` 发现的文件 stem，内存 relation name 来自 `tables` key；两个集合重名时构造失败。构造时复制 Mapping 结构并保留 Table/Catalog 强引用，relation 集合此后不可增删替换。每次 `query()` 创建独立短命 DataFusion Session，取得各 Table 的调用时快照并注册全部 relation；planning 后先校验 planned result Schema，通过后才扫描和 collect，并在返回 eager Table 前校验数据级约束。两次查询之间的 Table 追加只被后一次查询观察到；修改原 Mapping 不改变 Provider。`Scalar` 按 D50 接受 bool、Int64 范围整数、有限 float、str、bytes、aware datetime、`kat.WallClockTimestamp`、有限 Decimal 与 `kat.Duration`。迁移期 `ctx.sql(sql, **params) -> DataFrame` 只查询经过 Runtime 授权的旧 Dataset grant，不接受这里的 Table 或 Catalog，也不是新 Datasource 的融合入口。

标准 Workflow 返回值是：

```python
ds.Table
dict[str, ds.Table]
```

迁移期还接受旧 DataFrame，以及 `dict[str, ds.Table | DataFrame]`。dict 必须是非空的精确内建 dict。单值统一命名为 `main`；多个 Output 使用满足 KAT file-safe 规则的 dict key。

## 5. 端到端工作流

### 5.1 PACK 布局

```text
pack.toml
datasources/
  trace.py
  postgresql.py
workflows/
  correlate.py
tests/
```

### 5.2 文件 Provider

```python
# datasources/trace.py
import shutil
from datetime import datetime
from decimal import Decimal
from pathlib import Path

from kat import datasource as ds


TRACE_SCHEMA = ds.Schema({
    "events": {
        "observed_at": datetime,
        "thread_id": int,
        "name": str | None,
        "weight": Decimal,
    },
})


class TraceProvider:
    def __init__(self, *, source: Path, catalog_root: Path):
        self._source = source
        self._catalog_root = catalog_root
        self._fusion: ds.DataFusionProvider | None = None

    def decode(self):
        self._fusion = None
        if self._catalog_root.exists():
            shutil.rmtree(self._catalog_root)

        try:
            tables = TRACE_SCHEMA.create()
            for event in parse_trace(self._source):
                tables["events"].append(
                    observed_at=event.observed_at,
                    thread_id=event.thread_id,
                    name=event.name,
                    weight=event.weight,
                )
            ds.write(tables, destination=self._catalog_root)

            catalog = ds.open(tables={
                "events": self._catalog_root / "events.parquet",
            })
            fusion = ds.DataFusionProvider(catalog=catalog)
        except Exception:
            shutil.rmtree(self._catalog_root, ignore_errors=True)
            raise

        self._fusion = fusion
        return self

    def query(self, sql: str, *, params=None) -> ds.Table:
        if self._fusion is None:
            raise RuntimeError("trace source has not been decoded")
        return self._fusion.query(sql, params=params)
```

Parser 算法完全属于 PACK。它可以像上例一样向 Schema-created Table 逐行追加并用 `ds.write()` 同步落盘，也可以运行已有 binary Parser，再用 `ds.open()` 显式绑定其 Parquet 文件或 parts 目录。`decode()` 先清空 ready 状态，只在全部步骤成功后提交新的 backend；失败保持未准备并尽力清理独占目标。若 Parser 产生本地数据库，Provider 保留数据库并执行其原生 SQL，不整体转成 Parquet。

### 5.3 远端 Provider

```python
# datasources/postgresql.py
from kat import datasource as ds


class PostgreSQLProvider:
    def __init__(self, *, service: str):
        self._service = service

    def query(self, *, database: str, sql: str, params=None) -> ds.Table:
        # 连接、事务、只读策略、参数和资源关闭均由本 Provider 负责。
        arrow_table = execute_postgresql_as_arrow(
            service=self._service,
            database=database,
            sql=sql,
            params=params,
        )
        return ds.Table.from_arrow(normalize_postgresql_result(arrow_table))
```

Provider 在 Table 返回前已经完整执行远端 SQL 并关闭 query-local cursor、reader 与 transaction。`normalize_postgresql_result()` 表示该具体 Provider 在 bridge 前完成 D41 所需的来源类型规范化：有绝对时间语义的值转成 `timestamp(ns, tz="UTC")`；PostgreSQL `TIMESTAMP WITHOUT TIME ZONE` 没有绝对时间语义，Provider 不得猜成 UTC，必须由来源 SQL 或明确的领域规则解释，否则拒绝形成标准 Table。Provider 可以使用普通 Python context manager 复用连接池，但 KAT 不登记或关闭 Provider。

### 5.4 一个 Workflow 顺序查询两个 Database 并融合本地数据

```python
# workflows/correlate.py
from pathlib import Path
from tempfile import TemporaryDirectory

import kat

from kat.pack.datasources.postgresql import PostgreSQLProvider
from kat.pack.datasources.trace import TraceProvider


@kat.workflow(
    name="correlate",
    title="Correlate trace events with remote metadata",
    required_tables=[],
    parameters={
        "trace_path": "Trace 文件路径",
        "start_at": "Trace 查询窗口起点",
        "start_ns": "查询窗口起点",
    },
)
def correlate(
    ctx: kat.Context,
    trace_path: str,
    start_at: str,
    start_ns: int,
):
    with TemporaryDirectory(dir=ctx.datasource_root) as workspace:
        trace = TraceProvider(
            source=Path(trace_path),
            catalog_root=Path(workspace) / "catalog",
        ).decode()

        events = trace.query(
            """
            SELECT thread_id, COUNT(*) AS event_count
            FROM events
            WHERE observed_at >= $start
            GROUP BY thread_id
            """,
            params={"start": kat.WallClockTimestamp(start_at)},
        )

    # events 是 eager Table；临时 Parquet 此时已经可以删除。

    pg = PostgreSQLProvider(service="telemetry-service")
    telemetry = pg.query(
        database="telemetry",
        sql="SELECT thread_id, cpu_usage FROM observations WHERE ts_ns >= $1",
        params=(start_ns,),
    )
    owners = pg.query(
        database="control",
        sql="SELECT thread_id, process_name FROM process_registry",
    )

    fusion = ds.DataFusionProvider(
        tables={
            "events": events,
            "telemetry": telemetry,
            "owners": owners,
        },
    )
    summary = fusion.query(
        """
        SELECT
            o.process_name,
            SUM(e.event_count) AS event_count,
            AVG(t.cpu_usage) AS average_cpu_usage
        FROM events e
        JOIN telemetry t USING (thread_id)
        JOIN owners o USING (thread_id)
        GROUP BY o.process_name
        ORDER BY event_count DESC
        """,
    )

    return {
        "telemetry": telemetry,
        "summary": summary,
    }
```

PostgreSQL 的 `$1` 属于 Provider 来源 SQL；DataFusion Provider 的 `$start` 属于 KAT 本地 scalar 参数；`tables` 中的 relation name 只属于显式构造的 DataFusion Provider，不形成全局注册。`events` 与 `owners` 只作为中间融合输入，不落成 Output；`telemetry` 和 `summary` 因被 Workflow 返回而由 Runtime 发布。单源 Workflow 可以直接 `return provider.query(...)`，不构造 DataFusion Provider。

## 6. 已确认决定

### D1：来源特定的 Datasource Provider 由 PACK 拥有

Workflow 直接构造或取得 PACK 定义的 Provider，并显式调用其来源能力。目标公共模型删除 `kat.Provider`、`SourceExecutor` 与 `ctx.provider()`；KAT 不创建、继承限制或包装 Provider，也不规定所有 Provider 必须具有同一组方法。

Provider 仍可以组合 KAT 提供的 Schema、Table、`write()`、`open()`、Catalog 和本地查询能力，但这些能力是普通 Toolkit，不是第二个 Provider facade。

### D2：解析算法属于 Provider，KAT 标准化解析结果

用户按文件格式、协议和业务语义自行实现解析。KAT 不理解 Hitrace、Ftrace、文本、远端数据库或自定义二进制格式，也不要求解析器继承统一接口。

KAT 标准化的是解析结果链路：PACK 声明若干逻辑表及其列；解析代码按表提交数据；KAT 校验数据并落成可重新打开和查询的多表 Parquet Catalog。远端数据库可以直接执行自己的 SQL，并在需要落盘或融合时复用同一数据面。

### D3：公共 Datasource Schema 只使用 Python 类型描述表和列

PACK 作者用普通嵌套 Mapping 构造 `ds.Schema`，声明“产生哪些表、每张表有哪些列”；列只使用 Python 类型，不需要直接构造 `pyarrow.Schema` 或选择 `ds.UInt64` 等 KAT 类型标记。`ds.Schema` 复制并校验声明，还能创建与每张声明对应的可追加 `ds.Table`。Datasource Schema 是独立于物理位宽和编码的逻辑类型合同；Arrow 仍是 Table、Parquet 与 DataFusion 之间唯一的物理类型事实，不再增加平行的持久 Schema 格式。

首版 Schema 直接使用嵌套 Mapping：外层 key 是表名，内层 key 是保持声明顺序的列名，value 是 Python 类型。`bool`、`int`、`float`、`str`、`bytes`、D44 的 `datetime.datetime` 与 D45 的 `decimal.Decimal` 是首批逻辑类型，`T | None` 表示 nullable。Schema 创建的 Table 对基础 Python 值分别使用 Arrow Boolean、Int64、Float64、Utf8 与 Binary 作为规范物理编码，并按 `T` / `T | None` 产生 non-nullable / nullable 物理字段；datetime 与 Decimal 使用各自固定规范编码。它们不根据首批值推断位宽、有无符号、时间语义、Decimal scale 或空值约束，也不在 Python 类型之间隐式转换。其他逻辑类型只在真实需求时增量加入；这不限制高级来源通过 D26 的 Arrow bridge 形成 D41 准入的 Table。

Schema 只描述解析结果，不描述解析算法。列类型、空值约束与顺序必须在追加数据前确定，使零行 Table 也具有稳定结构。Schema 至少包含一张表、每张表至少一列；`schema.create()` 返回普通 `dict[str, ds.Table]`，key 与声明表集合及顺序一致。KAT 不增加公共 `TableSchema` 或 Builder 类型。

### D4：`ds.write()` 直接把 Table Mapping 写成 Parquet

多表落盘使用同步模块函数 `ds.write(tables, destination=...) -> None`，其中 `tables` 是非空 `Mapping[str, ds.Table]`；不公开 Writer 对象，不使用 `ctx.datasource.write(...)`，也不读取隐式当前 Workflow Context。`destination` 必须由调用方显式提供；生产 Workflow 的临时 Datasource 物化必须位于从 `ctx.datasource_root` 创建的当前 Workflow workspace 内，普通 Python 与 PACK 测试则可以直接使用临时目录。

Mapping key 是本次 Parquet 物化的逻辑表名并形成 `<name>.parquet`，Table 自身不携带名称。`ds.write()` 使用调用开始时每张 Table 的快照，负责 Parquet 写入与零行表保留；Table 已有 Arrow buffers 在交给 PyArrow Parquet Writer 前不做 KAT 整表复制，Parquet 编码和压缩不属于零拷贝承诺。它不解释来源配置、决定复用策略或发布 Run Output；成功后原 Table 保持可用并可继续 append。

### D5：Schema 创建 Table，Table 原子追加 Python 行

PACK 作者用 `schema.create()` 一次取得与全部声明对应的普通 `dict[str, ds.Table]`，再以 `table.append(**row_values)` 逐行提交 Python 标量。调用必须恰好提供全部列，Table 在修改任何内部缓冲前校验整行；失败不留下半行。用户不创建平行的列列表 Mapping、Arrow Builder 或 batch 对象。

Table 自己维护待转换的 Python 行并按 KAT 固定实现批次形成 Arrow chunks，不调用用户对象转换方法，也不做跨批次类型推断。`ds.write()` 直接消费这些 Table；声明但从未 append 的零行 Table 仍产生携带声明 Schema 的零行 Parquet 文件，使空数据与缺失/失败产物可区分。

### D6：Schema 统一逻辑结果，不强制 Parser 使用 Parquet

`ds.write()` 是自定义 Python Parser 的默认落盘方案，不是所有 Parser 的强制出口。已有 binary Parser 若产生锁定 DataFusion 可以扫描的多张 Parquet relation，Provider 可以通过 KAT Toolkit 原地打开，不先声明 Datasource Schema，也不先解码再编码；list、struct、date、duration 等宽列在 Fusion SQL 中显式投影、展开或 cast 后再形成 D41 标准 Table。若 Parser 产生 SQLite 或其他数据库，Provider 保留该数据库及其索引和查询能力，不在 `decode()` 后整体转换为 Parquet。

数据库型 Provider 使用来源自己的 SQL 方言与执行器；某个具体查询选择标准结果路径时，才把该结果形成可供 Python 直接读取、返回为 Run Output 或参加多数据源融合的 `ds.Table`。KAT 不转换整个数据库、不规划来源内 SQL，也不把整个数据库注册到融合 DataFusion。

### D7：`ds.Table` 是 Schema 创建和查询结果共用的可追加单表值

KAT Datasource Toolkit 用同一个 `ds.Table` 表示 Schema 创建的解析目标和已经执行完成的查询结果。Table 始终具有确定的单表列结构，可以追加满足其 Schema 的数据并可重复读取；它不是远端 cursor、惰性 SQL plan、Parquet 路径或已经注册到融合 Session 的 relation。普通使用不要求 PACK 作者理解 Arrow，高级来源适配器仍可通过显式 bridge 保留 D41 准入的驱动 Arrow 类型。

追加合同由构造路径决定：`Schema.create()` 与 `Table(single_table_schema)` 按 Python 逻辑 Schema 校验，`Table.from_arrow()` 与 `DataFusionProvider.query()` 按其已经准入的 Arrow 物理 Schema 校验；Source query 可以选择前述任一路径。所有 Table 都使用同一个原子 `append(**row_values)`，不增加只读变体。向查询结果追加只改变该结果 Table，不反向修改查询输入、Catalog 或此前结果。Workflow 可以直接返回 `ds.Table` 形成单源 Output，也可以在后续显式步骤中把它交给 KAT 本地数据面参加融合。

读取、Source query、Fusion query、显式 Parquet 写入与 Workflow Output 发布都观察调用开始时 Table 已有内容的快照。形成快照时 Table 把尚未转换的尾批次转成 Arrow，并通过 Arrow chunks 共享既有 buffers，不复制历史整表；调用完成后 Table 仍可继续 append。后续追加不改变已经开始的查询、既有查询结果、已经写出的 Parquet 或已经发布的 Run Output。首版不承诺同一 Table 的并发 append 与读取，普通 Workflow 必须按顺序调用。

本决定取代 PR #228 中把 `kat.Table` 定义为已经落盘、具名并绑定当前 operation 的不可构造句柄的模型。融合 relation 的创建、命名与生命周期必须使用另一个显式步骤，不能继续藏在 `Provider.query()` 的副作用中。

### D8：DataFusion Provider 只使用显式 relation

多数据源融合使用 Workflow 显式构造的 `ds.DataFusionProvider`。Provider 只接收调用方明确命名的内存 `ds.Table`、磁盘 Parquet relation 或两者组合；名称只在该 Provider 的本地 SQL 中有效。Datasource Provider query 不接收本地结果名、不自动注册，也不留下全局 catalog 状态。

DataFusion Provider 在查询调用开始时分别取得所有显式 Table 的当前快照；查询执行期间或完成后的追加不会改变该次输入。下一次查询同一 Table 时才观察其新内容。Parquet relation 由 DataFusion 直接扫描，不先整体加载到内存。

该接口不增加 `ctx.register()`、`ctx.table()`、Binding 或新的 operation-bound Table 句柄。SQL 引用未显式提供的 relation 时直接失败，不发现来源 Provider、不访问来源 catalog，也不隐式执行来源 SQL。同一个 `ds.Table` 可以直接成为 Run Output，同时作为一次或多次 Fusion query 的输入。

迁移期 `ctx.sql(sql, **params) -> DataFrame` 只保留旧 Dataset、`required_tables`、Table Grant 与 Execution Lease 的兼容语义；新 Datasource 不通过该接口融合。具体迁移边界由 D48 与 ADR-0066 记录。

### D9：Source query 与 Fusion query 都形成 eager `ds.Table`

`DataFusionProvider.query()` 在调用期间把显式 relation 注册到内部 DataFusion Session、执行完整 SQL 并把结果收集为可重复读取的 `ds.Table`。它不把 DataFusion `DataFrame` 作为新的 Datasource 标准查询结果。来源 `Provider.query()` 与本地 `DataFusionProvider.query()` 因而向 Workflow 呈现同一种结果值，结果可以继续作为另一条 Fusion query 的显式输入、由 Python 重复读取或直接成为 Run Output。

eager `ds.Table` 需要结果驻留内存，这是可重复读取且不依赖重读 Parquet的直接代价；首版不再同时提供 lazy/streaming Query Result 变体。Provider 可以在来源内使用数据库或本地 DataFusion 的流式执行来控制中间数据，但进入标准 Query Result 后必须完整形成 `ds.Table`。

迁移期 `ctx.from_arrow()`、`ctx.sql(sql, **params) -> DataFrame` 与 DataFrame Output 原样服务尚未迁移的 Dataset/DataFrame authoring path；新 Datasource 的 DataFusion Provider 始终返回 Table。是否最终删除这些旧 DataFrame authoring path 属于后续迁移计划，不影响本文目标模型。

### D10：Parquet Catalog 同时支持显式表路径与根目录发现

已有 Parquet Parser 可以通过 `ds.open(tables={logical_name: path})` 显式绑定物理布局；非空 Mapping 中的每个 path 都必须存在，并且可以是一张表的 Parquet 文件或包含该表多个 part 的目录。因此，知道预期表集合的 Provider 可以通过显式绑定发现缺表。parts 目录递归收集所有 `.parquet` 普通文件、忽略其他扩展名，并要求至少存在一个 part；目录层级只组织文件，不从 `key=value/` 路径推导 Hive partition 列。KAT 原地读取 metadata、采用文件的实际物理 Schema 并校验这些路径，不移动、复制或重新编码，也不要求 Parser 生成 KAT Manifest；数据行只在 DataFusion Provider 扫描时读取。

对于标准的平铺多表目录，`ds.open(root=directory)` 自动把目录当前已有的每个 Parquet 文件解释为一张逻辑表，表名取文件 stem。例如 `events.parquet` 与 `threads.parquet` 形成 `events`、`threads` 两张表。`root=` 是发现模式，只证明当前发现的非空 relation 集合自洽，不知道调用方是否原本期待另一张表；它不把目录内所有文件合并成一张分片表。单张分片表或需要验证预期集合的多表产物使用显式 `tables={name: path}` 表达。

`root=` 首版只扫描该目录直属的普通 `.parquet` 文件，不递归子目录；其他扩展名文件忽略。嵌套 Parquet 产物必须通过 `tables=` 显式绑定，避免相对路径到表名的额外编码规则和不同子目录中的 stem 冲突。`root` 与 `tables` 必须且只能提供一个，并且发现或显式绑定的 relation 集合必须非空。

`ds.write(tables, destination=...)` 产生一表一文件、可由 `root=` 直接打开的平铺默认布局。两种打开方式都以 Parquet footer 为物理 Schema 事实，不接收或比较 Datasource Schema，也不自动重排、补列、删列或 cast。显式 parts 的列名、顺序、物理类型与 nullability 必须一致；Arrow field/schema metadata 不构成一致性合同。不增加独立 Manifest。

### D11：`ds.Catalog` 只描述经过校验的 Parquet relation

`ds.open(...)` 返回可重复使用的 `ds.Catalog`。打开只读取 Parquet metadata、取得并校验实际物理 Schema、保存具名表路径，不创建 DataFusion Session，也不读取全部数据行。Catalog 是对原路径的只读视图，调用方必须在使用期间保持文件存在且内容不变；它不提供 `query()`，只能作为 `catalog=` 显式传给 `ds.DataFusionProvider`。

Catalog 只公开按名称排序的 `catalog.tables`，不公开 `schema()`、Relation wrapper 或 `pyarrow.Schema`。Workflow 若要查看列结构，使用同一个 DataFusion Provider 执行 `DESCRIBE relation_name`；该命令的 `column_name`、`data_type` 与 `is_nullable` 结果按普通 D41 Table 返回。`SHOW TABLES` 仍是允许的 DataFusion SQL，但会包含 information schema 条目，不作为 Catalog 自有 relation 列表的替代合同。

本地文件 Provider 可以持有 `DataFusionProvider(catalog=catalog)` 并把自己的 `query()` 委托给它，从而不重复实现 DataFusion Session、路径注册、参数绑定和结果转换。远端数据库以及数据库型 Parser 产物仍由对应 Provider 使用来源自己的 SQL 方言、参数和执行器；它们不伪装成 Parquet Catalog。该决定由 ADR-0067 记录。

### D12：Provider 生命周期使用普通 Python 规则

KAT 不登记 Provider、不要求 `close()` 协议，也不在 Workflow 结束时自动回收 Provider。eager `Provider.query()` 必须在返回 `ds.Table` 前关闭该次 cursor、reader、临时进程和其他 query-local 资源；来源错误和关闭错误由该 Provider 自己保留正确的主错误语义。

需要复用连接池、客户端或外部进程的 Provider 可以按普通 Python 惯例实现 context manager，由 Workflow 显式使用 `with Provider(...)`。不需要长期资源的 Provider 是普通对象。`ds.write()` 是同步函数，调用返回时 Parquet writer 已全部关闭；首版 `ds.Catalog` 只持有不可变路径和从 footer 取得的物理 Schema 描述，不提供 `close()` 或 context manager，也不形成必须由 KAT 编排的资源协议。

首版文件 Provider 的 workspace 生命周期由 Workflow 的普通 `TemporaryDirectory(dir=ctx.datasource_root)` 管理，而不是由 Runtime 自动 close Provider。Workflow 必须在 workspace 存活期间完成所有依赖 Catalog 的查询；eager Table 返回后已经与来源文件脱离，退出 `with` 即可清理。Provider 不得在 workspace 删除后继续 query。

### D13：PACK 新增顶层 `datasources/` 生产模块

PACK 固定源码布局新增可选顶层 `datasources/`，用于放置由该 PACK 拥有的 Provider、Schema 与解析实现。其规范 Python identity 是 `kat.pack.datasources.*`，与 `kat.pack.workflows.*`、`kat.pack.helpers.*` 并列；Workflow 通过普通 Python import 使用其中的 Provider。

这项决定有意修改 ADR-0017 与 ADR-0047 中“生产模块只有 workflows/helpers 两组规范身份”的边界。Datasource 虽然仍是普通 Python 代码，但其来源合同与复用价值已经足以成为 PACK 的一等代码区域；继续放在 `helpers/` 会把稳定来源能力误表达成无领域身份的通用 helper。

Runtime 只把该目录作为 `kat.pack.datasources.*` 规范 namespace 挂载，服从 Python 标准 import；KAT 不扫描、不预加载、不注册 Provider，不要求一文件一个 Provider，也不从 module、类名或 `pack.toml` 推导来源身份。未被 Workflow import 的 Datasource 不在 inspection 时执行，普通 `__init__.py` 与 namespace package 语义均由 Python 决定。该决定由 ADR-0063 记录。

### D14：`ds.Table` 是最小可追加 Python 数据容器

`ds.Table` 首版只提供原子 `append(**row_values)`、`len(table)`、返回稳定列名 tuple 的 `table.columns`、按列返回与内部存储隔离的 Python tuple 的 `table[name]`，以及返回新 `list[dict[str, object | None]]` 的 `table.to_rows()`；同一值可以被反复读取和继续追加。调用方修改返回的 tuple、list 或 dict 不改变 Table；后续读取只因显式成功 append 而变化。普通计算使用 Python 对这些值操作，关系选择、过滤、连接与聚合通过把 Table 显式传给 DataFusion Provider 完成。D42 的高级 Arrow 输入只读合同不改变这些 Python 读取语义。

除完整行 `append()` 外，Table 不提供更新/删除、`filter()`、`select()`、`group_by()`、表达式系统或其他 DataFrame API。内部 Arrow Table 不是普通计算接口，只能经 D26/D43 的显式高级只读 bridge 进出；限制这一职责避免 KAT 重建 pandas、PyArrow compute 或 DataFusion 已有的通用计算层，也保证同一 Table 作为 Python 输入、Run Output 和 Fusion query 输入时保持一致。所有 Table 都要求列名是非空且唯一的字符串；手工构造、Arrow bridge、Source query 或 Fusion query 产生空名或重名列时直接失败，SQL 作者必须使用 `AS` 消除歧义。

### D15：所有 Table 来源共享同一个追加合同

普通 Python 作者通过 `ds.Table(single_table_schema)` 或 `ds.Schema.create()` 创建空 Table，再逐行调用 `append()`；不提供模块级 `ds.table()`、columns/rows 批量 constructor 或另一套 Builder。已有 Arrow 结果通过 `ds.Table.from_arrow()` 接入，DataFusion Provider 直接产生同一个 Table 类型。

每次 append 必须恰好提交完整一行，并在任何状态修改前验证列集合、值类型、物理范围和 nullability；失败不留下半行。Schema-created Table 使用 D3/D39 的 Python 逻辑合同，Arrow/query Table 使用 D41/D49 已准入的实际物理合同；两者都不因来源切换公共方法或产生隐藏只读状态。

### D16：Workflow 返回 Table 决定 Run Output

新的标准 Output 合同是 `ds.Table | dict[str, ds.Table]`。单个 Table 规范化为 `{"main": table}`；dict 必须非空且 key 继续使用 KAT 既有可移植 Output name 规则，并成为 Run Manifest 与 Output Query 中的逻辑名称。Table 自身不携带 Output name，Provider query 也不接收 `name`。不接受任意自定义 Mapping，避免 Runtime 规范化期间观察到动态键值。

Runtime 只在 Workflow 成功返回后把选中的 Table 写成最终 Run Output Parquet，不重新执行来源 SQL。仅作为 Fusion query 输入而未返回的 Table 不产生 Output 文件；同一个 Table 可以先参加融合，再与融合结果一起通过不同 dict key 返回。迁移期继续接受单个 DataFrame 以及非空 `dict[str, ds.Table | DataFrame]`，允许两种值混合；单个旧 DataFrame 同样使用 `main`。空 dict、其他 Mapping 或其他返回类型直接失败，但 `ds.Table` 是 Datasource Toolkit 的新标准值。

### D17：数据库接入统一在 Table 结果边界

KAT 首版不提供通用 DB-API、ADBC 或数据库 executor helper。数据库 SQL 方言、参数、连接与凭据、事务和只读保证、驱动类型转换及资源关闭都由具体 PACK Provider 拥有；Provider 可以创建 `ds.Table(result_schema)` 后逐行追加普通 Python 结果，也可以把驱动已经产生的 Arrow Table 交给 D26 的 `ds.Table.from_arrow()`，形成相同的标准结果。

这避免为了不同数据库的差异重新建立与 SourceExecutor 等价的抽象。仓库可以提交一个使用具体驱动的可运行 `PostgreSQLProvider` 范例，证明远端查询到 `ds.Table` 再到 `DataFusionProvider.query()` 的完整链路，但该范例及其驱动不提升为 KAT 核心协议。以后有多个真实 Provider 证明某段数据库胶水稳定复用时，再提炼为可选 Toolkit。

### D18：同步 `ds.write()` 失败尽力清理，崩溃残留由 Provider 重建

`ds.write(tables, destination=...)` 要求 destination 开始时不存在，并在一个同步调用中依次写出调用时快照。任一 Table 验证、Parquet 编码、文件写入或 writer close 失败时，函数先尽力关闭已打开资源并删除本次创建的整个目标目录，再抛出首个错误；清理错误只作为附加诊断，不覆盖主错误。没有可被调用方继续使用的公共 Writer 或 poison 状态。

进程崩溃可以留下不完整临时目录。首版新 Workflow 不发现或复用这些旧目录，而是创建新的临时 workspace；Provider 重试 `decode()` 时先清空 ready 状态并删除明确交给该实例独占的旧目标。`ds.open()` 仍重新验证当前可见路径、Parquet footer、DataFusion 扫描类型与分片一致性；需要检测本次预期缺表的 Provider 使用 `open(tables={expected_name: expected_path})`。KAT 不引入 Datasource Schema、Manifest、`_SUCCESS`、自动覆盖、合并、恢复或修复协议。

### D19：Catalog 采用 DataFusion 可扫描的来源物理 Schema

`ds.open()` 不把已有 Parquet 的列映射回 Python Datasource Schema，也不要求已有 Parser 使用 `ds.write()` 或 D41 的规范物理编码。它直接从 footer 取得并保留锁定 DataFusion 可以扫描的 Arrow/Parquet 物理 Schema；除标准标量外，首版至少验证 list、struct、date 与 duration 可以作为 Catalog 来源列。打开不构造标准 Table，也不修改或重新编码来源文件。

首版 Catalog 的锁定扫描集合包括 Arrow 标准标量、date/time/timestamp/duration/decimal、递归 list/fixed-size-list/large-list、struct、map 与 dictionary；extension、union、interval、run-end encoding、list-view 及其他未列类型拒绝。该集合由 footer 递归准入固定，不通过构造全局或短命 DataFusion Session 探测。

同一张显式分片表的所有 Parquet part 必须具有一致的物理 Arrow Schema，使 DataFusion 可以把它们作为一张关系读取；一致性比较忽略无业务语义的 Arrow field/schema metadata。`ds.write()` 采用 D3、D44 与 D45 的规范编码；这些选择只是新写 Python 数据的确定默认值，不是 binary Parser 的接入格式要求。Catalog 宽列只有在 SQL 最终结果被投影、展开或 cast 为 D41 类型后才能形成 `ds.Table`。

### D20：Provider 不接收 Workflow Context

Datasource Provider constructor 和方法只接收真正需要的普通来源配置、Schema 与路径，不接收或保存 `kat.Context`。需要文件 backend 时，Workflow 在 `ctx.datasource_root` 下创建临时 workspace，再把其中具体目标路径传给 Provider；远端数据库 Provider 只接收其连接配置。Datasource Provider 不能调用兼容 `ctx.sql()`、访问 Run workspace 或发布 Output；它可以像普通调用方一样组合 `ds.DataFusionProvider`。

Context 继续只拥有迁移期旧 Dataset execution plane 与 PACK 存储根等运行能力，不再承担新 Datasource Fusion query、Provider factory 或 Datasource Toolkit namespace。这个边界让同一个 Provider 可以在普通 Python 和 PACK test 中直接使用 `tmp_path` 验证，也避免 Provider 生命周期重新绑定 Execution Lease。

### D21：典型文件 Provider 显式准备内部 backend

推荐的文件 Provider 是一个实例对应一个具体来源的有状态普通类。`decode()` 入口先清空 ready/backend，并删除调用方明确交给该实例独占的旧目标；随后显式执行自定义 Python Parser 或已有 binary Parser。若产物是 Parquet，它在同步 `ds.write()` 或 binary Parser 成功返回后显式调用 `ds.open()`。Catalog、数据库路径、DataFusion Provider 等均先保存在局部变量，只有 parse、write、open、预期 relation 校验和 backend 构造全部成功后才一次提交为实例状态。

任何 decode 失败都保持未准备并尽力清理本次目标，不回退旧 Catalog；`query()` 在未准备时直接报错，后续可以重新 `decode()`。同一 Provider 或同一独占目标首版不支持并发 decode/query。`decode()` 返回 `self` 以允许可选链式调用，但 Workflow 可以分两步显式调用。Parquet Provider 可以用 `ds.open()` 取得 Catalog，再保存 `ds.DataFusionProvider(catalog=catalog)` 并把自己的 `query()` 委托给它；Catalog 自身没有查询能力，也不要求暴露给普通 Workflow。无需解析的远端数据库 Provider 构造后即可 query。这是示例和作者约定，不是 KAT 用反射检查的 Provider 协议。本决定由 ADR-0069 记录。

### D22：每次 DataFusion Provider query 使用隔离 Session

DataFusion Provider 构造时复制并冻结 relation Mapping 结构，保存 Table 与 Catalog 强引用，不提供 register/deregister/replace。每次 `query()` 创建独立、短命的 DataFusion Session，只注册该 Provider 的内存 Table 调用时快照、Catalog relation 及必要 Session 配置，完整执行并形成 `ds.Table` 后释放。它不接收迁移期旧 Dataset grant；不同 Provider 可以复用 relation name，不存在全局 mutable catalog。

前一次结果若要作为新的 relation 继续参与计算，Workflow 必须用新的 `tables` Mapping 显式构造 DataFusion Provider。SQL 失败只结束当前调用，不 poison Provider 或 Context；同一 Provider 可以换一条 SQL 重试。PACK 仍不能取得底层 SessionContext。本决定取代 PR #228 为整个 operation 维护 Provider Table catalog、名称保留和失败 poison 状态的设计。

### D23：Fusion relation 与 scalar 参数分离

DataFusion Provider 的 relation 配置与 `query(sql, *, params=None) -> ds.Table` 的 scalar 参数是两个显式输入面：前者把本地 relation name 绑定到 Table 或 Parquet，后者为当前 SQL 提供 DataFusion `$name` scalar 参数。接口不把 scalar 作为 `**params` 接收，避免动态参数名与 Provider 配置或未来 keyword 冲突；调用开始时复制参数 Mapping，再按 D50 规范化每个值。

Scalar 使用 Table 数据面已有的 Python 值族，并由 KAT 转成明确的 DataFusion/Arrow scalar，不直接暴露 `pyarrow.Scalar` 或依赖 DataFusion 的隐式类型猜测。SQL 文本和参数 Mapping 只供本次调用使用，不记录到 Table、Provider 或全局 catalog。

### D24：Fusion relation 与 parameter 使用可移植名称

内存 `tables` key 与 `params` key 都必须满足小写 SQL-friendly 名称规则 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`。Catalog relation name 已经由 D31 的 file-safe table name 规则校验；构造 DataFusion Provider 时，Catalog 与内存 Table 的 relation name 并集必须唯一，重名立即失败。Relation 与 parameter 属于不同 namespace，可以使用相同文本。KAT 不复制 DataFusion SQL 关键字列表，罕见关键字冲突由 PACK 在 SQL 中正确引用或改名。

DataFusion Provider 在创建 Session 前检查 Catalog 与内存 relation 重名，不允许覆盖或 shadow。SQL 引用不存在的 relation 或 parameter 只使本次查询失败，不触发 Datasource Provider、来源发现或 Context poison。迁移期旧 Dataset grant 只属于兼容 `ctx.sql()`，不与这些 relation 合并。

### D25：`ctx.sql()` 只保留旧 Dataset 兼容职责

新 Datasource 不修改 `ctx.sql(sql, **params) -> DataFrame` 的签名、参数或惰性返回类型，也不把 Table、Parquet 或 DataFusion Provider 绑定进 Context。该方法在迁移期只为旧 Dataset、`required_tables`、Table Grant 与 Execution Lease 保留兼容职责；Runtime 继续在这里做授权与缺表检查。

新 Workflow 直接显式构造 `ds.DataFusionProvider`；公共 Provider 不接收 Context、Lease、Resolved Dataset 或裸 grant 路径。旧 Dataset 使用者迁移完成后再整体删除 `ctx.sql()`，不为新 Datasource 保留第二套融合入口。本决定由 ADR-0066 记录并取代 ADR-0064 的破坏性迁移方案；ADR-0005、ADR-0032 与 ADR-0033 中的旧 Dataset SQL/DataFrame 合同继续有效。

### D26：高级来源通过 Table classmethod 从 Arrow 构造标准 Table

Datasource Toolkit 提供 `ds.Table.from_arrow(table: pyarrow.Table) -> ds.Table`。数据库驱动、binary Parser 或其他高级适配器已经取得 Arrow Table 时，可以通过该入口保留 D41 准入的列名、顺序、物理类型与 nullability，包括 `timestamp`、`decimal` 等丰富类型；Arrow field/schema metadata 不属于标准 Table 合同。调用方不需要先转成 Python rows，也不需要为动态查询结果手工重建 Datasource Schema。若来源交付 `RecordBatchReader`，Provider 先完整读取为 Arrow Table，再调用该入口，因此返回值仍满足 D9 的 eager、可重复读取合同；buffer 所有权遵守 D42。返回 Table 按这份物理 Schema 支持与其他 Table 相同的 `append()`。

这一高级 bridge 不扩大普通 `ds.Schema`、`Table.append()` 与 `ds.write()` 首版支持的七种 Python 逻辑类型，也不要求普通 Parser 作者理解 Arrow。由 bridge 或 DataFusion Provider query 形成的 `ds.Table` 可以承载 D41 中经验证的更多结果类型；Python 列读取、融合与 Output publisher 必须保持这些类型。既有 `ctx.from_arrow()` 暂时保留原有 DataFrame 语义，与返回标准 Table 的 `ds.Table.from_arrow()` 是两个显式入口，不按输入动态切换。

### D27：已有 Parquet 的 nullability 直接采用 metadata

`ds.open()` 没有对应的 Python 逻辑必填或可空声明，因而直接保留 footer 中每个字段的 physical nullability。KAT 不扫描数据页来证明一个 nullable 字段当前恰好没有 null，也不把它提升为 non-nullable；显式 parts 的 nullability 必须和其他物理字段属性一起保持一致。

这一规则与 D11 的 metadata-only open 保持一致，使 Catalog 结构只依赖 footer 而不依赖某一批文件内容。实际查询若遇到数据页损坏仍在扫描时失败。

### D28：Datasource Toolkit 只通过 `kat.datasource` 暴露

KAT 的标准数据面模块是 `kat.datasource`，PACK 推荐使用 `from kat import datasource as ds`。模块公开 `Schema`、`Table`、`Catalog` 以及经本文确认的数据追加、Arrow bridge、Parquet 写入和打开函数。本文中的 `ds.*` 均是这个规范模块的别名，不是 PACK 下 `datasources/` 源码目录的对象。

新类型与函数不再平铺为 `kat.Table`、`kat.Schema` 等顶层名称。PR #228 引入的顶层 `kat.Provider`、`kat.SourceExecutor`、`kat.ParquetSource` 与 operation-bound `kat.Table` 按 D1 删除，不保留双入口或兼容 alias。PACK 的 `kat.pack.datasources.*` 拥有来源实现，KAT 的 `kat.datasource` 只提供可组合 Toolkit，两者不会互相扫描、注册或包装。

### D29：Table 落盘后仍由调用方显式 `open`

`ds.write()` 同步写完 Table Mapping 后返回 `None`，不返回、不缓存也不隐式创建 Catalog；当前 Workflow 已持有的内存 Table 可以直接读取、融合或返回。后续进程需要查询物化的 Parquet 时，Provider 显式调用 `ds.open(root=destination)`，并自行决定是否保存返回的 `ds.Catalog`。

`ds.write()` 返回后不保留输入 Table 或本次 Arrow 快照。常见 Provider 在 `decode()` 的局部作用域构造并写出 Tables，随后返回只引用 Parquet 路径的 Catalog；函数返回且没有其他强引用时，Table 与其 Arrow buffers 按普通 Python 生命周期释放，不要求用户显式 `del`。`del` 只是在长函数中提前删除某个变量绑定，不能释放仍被其他 Table、Arrow snapshot 或查询结果引用的 buffers。

因此刚写出的产物与已有 Parser 产物通过同一个 `open()` 边界完成路径、footer、DataFusion 扫描类型与分片一致性校验。Table、Parquet 物化与 Catalog 的生命周期保持独立，不存在 context manager 退出值或 after-close 隐藏状态。

### D30：Datasource Schema 是不可变的 Table 构造合同

`ds.Schema(...)` 在构造时复制并冻结输入声明，调用方随后修改原始 Mapping 不影响 Schema。它约束 `schema.create()` 产生的 Table 集合、列名、列顺序、Python 逻辑类型与 nullability；`ds.open()` 不接收、保存或比较这份 Schema。

KAT 不在打开时自动重排、补列、删列或 cast。已有 Parser 的每张 relation 以自身 footer 为事实；单张分片表的所有 part 必须具有一致物理 Arrow Schema，比较时忽略 Arrow field/schema metadata，但不忽略字段顺序、物理类型与 nullability。

### D31：只对路径与 SQL identity 施加必要名称约束

Datasource Schema 的表名通常成为 `ds.write()` 默认布局中的 `<table>.parquet`，因此复用既有 Output/table name 规则：满足 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，并排除 `con`、`prn`、`aux`、`nul`、`com1`—`com9` 与 `lpt1`—`lpt9` 等 Windows device name。Schema table Mapping、`ds.write()` Mapping、`ds.open(tables=...)` 的 key 以及 `ds.open(root=...)` 取得的文件 stem 都使用这一规则；root 中出现不合法 stem 时直接失败，不清洗或跳过。

列名不形成路径，只要求是非空字符串并按大小写精确匹配，不强制 snake_case，也不清洗或规范化已有 Parquet 列名；非简单标识符在 SQL 中由 PACK 正确引用。Fusion relation 与 parameter 按 D24 使用小写 SQL-friendly 正则，但由于不落成文件，不额外排除 Windows device name。Output name 继续使用完整 file-safe 规则。

### D32：Table 提供追加与隔离读取

`ds.Table` 通过 `table.append(**row_values)` 逐行追加既有列合同约束的 Python 标量，并提供 `len(table) -> int`、`table.columns -> tuple[str, ...]`、`table[name] -> tuple[object | None, ...]` 与 `table.to_rows() -> list[dict[str, object | None]]`。每次 append 必须恰好提供全部列；Table 在修改任何内部状态前完成整行的列集合、值类型、物理范围与 nullability 校验，因此失败不会留下半行。Schema-created Table 的合同来自 Python Schema，Arrow/query Table 的合同来自其 D41 物理 Schema。Table 自己维护并按 KAT 固定实现批次把待写 Python 行转换为 Arrow batch，用户不创建或配置列列表、batch size 或 Builder。

`to_rows()` 每次创建新的 list/dict，列读取也不暴露内部可变存储；调用方修改返回容器不改变 Table。UTC nanosecond timestamp 投影为保留九位小数精度的 `kat.WallClockTimestamp`，Decimal128/Decimal256 投影为 `decimal.Decimal`，不经 Python `datetime` 或 float 中转。

所有 Table 都要求列名是非空且唯一的字符串。Schema 创建、Arrow bridge 与 DataFusion Provider query 形成重名或空名列时，在返回标准 Table 前失败；SQL 查询需要使用 `AS` 明确消除重名。除 Schema 约束的数据追加外，Table 不提供列结构 mutation、filter/select/group-by、表达式或其他 DataFrame API。

### D33：Catalog 依赖稳定源路径，Table 与源路径脱离

`ds.Catalog` 是 `ds.open()` 所绑定原 Parquet 路径的只读 live view，不复制文件或形成快照。调用方必须在 Catalog 及使用它的 DataFusion Provider 存活期间保持文件存在且内容不变；外部删除或修改路径属于调用方违约，KAT 不检测也不保证后续查询结果，查询可能失败，也可能读取到同路径的新内容，且不会缓存旧文件内容来维持快照。

DataFusion Provider 从 Catalog 扫描后返回的 eager `ds.Table` 已经与 Catalog、短命 DataFusion Session 和源文件脱离；查询完成后释放 Catalog 或删除源文件，都不影响既有结果 Table 的 Python 读取、后续 Fusion query 或 Output。首版 Catalog 不提供 `query()`、`close()` 或 context manager，也不持有 Session。

### D34：显式 parts 目录递归聚合一张表

`ds.open(tables={name: path})` 中的 path 可以是单个 Parquet 文件，也可以是一张逻辑表的 parts 目录。parts 目录递归收集后缀为 `.parquet` 的普通文件并按稳定相对路径顺序校验，忽略其他扩展名文件，且至少必须包含一个 part；所有 part 继续遵守 D19/D30 的一致物理 Schema 合同。

目录名只用于组织文件，KAT 不从 `date=.../`、`cpu=.../` 等层级推导 Hive partition 列。该模式与 `root=` 的平铺多表发现严格区分：`root=` 只读取直属 Parquet 文件并把每个文件 stem 解释成独立表。`root` 与 `tables` 必须且只能提供一个；发现或显式绑定的 relation 集合必须非空。

### D35：DataFusion Provider 是唯一的新 Datasource 本地 SQL 入口

`DataFusionProvider.query()` 使用 KAT 固定版本的 DataFusion SQL admission：只接受一条只读语句，允许 `SELECT`、`WITH`、`VALUES`、`DESCRIBE`、`EXPLAIN`，以及结果物理类型满足 D41 的只读 `SHOW` 变体；拒绝多语句、DDL、DML、`COPY` 与 Session 状态修改。DataFusion 54 的 `SHOW FUNCTIONS` 会产生 `list<string>` 列，因此即使语句只读，也在标准 Table admission 处拒绝。scalar 转换与参数名称遵守 D23/D24。

一次调用的顺序固定为：复制并规范化参数、校验单语句只读形态、创建短命 Session、取得内存 Table 快照并注册所有 relation、完成 SQL planning、按 planned result Schema 校验 D41 的列名/唯一性/物理类型/nullability、执行并 collect、按实际 Arrow arrays 校验 non-nullable 列总 `null_count` 等数据级约束，最后构造 `ds.Table`。planned Schema 失败时不得开始 Parquet 或内存 relation 的数据扫描；collect 或最终数据校验失败只结束当前调用，不 poison 可复用 Provider。

本地 SQL 的解析、planning 或执行失败只结束当前调用，不 poison DataFusion Provider，后续可以使用其他 SQL 重试。这一准入只约束 KAT 拥有的新 Datasource 本地 DataFusion；迁移期 `ctx.sql(sql, **params) -> DataFrame` 保持旧 Dataset 准入与惰性执行合同，远端或数据库 `Provider.query()` 的方言、允许语句、事务和读写策略仍完全属于来源 Provider，KAT 不预先解析其 SQL。

### D36：Workflow 用单值或非空普通 dict 选择 Output

新的标准 Workflow 返回值是 `ds.Table | dict[str, ds.Table]`。单个 Table 规范化为 `{"main": table}`；dict 必须是精确内建 `dict`、必须非空，并用满足完整 file-safe Output name 规则的 key 命名每个 Table。Table 自身没有 Output name，Runtime 只在 Workflow 成功返回后物化这些最终选择，不为中间 Table 提前创建 Output。

迁移期继续接受单个旧 DataFrame，以及非空的精确 `dict[str, ds.Table | DataFrame]`，允许新旧值混合；单个 DataFrame 也规范化为 `main`。任意自定义 Mapping、空 dict 或其他值失败，避免 Output 规范化期间观察动态键值，也不增加“零 Output Workflow”语义。

### D37：Schema 创建与声明一一对应的 Table Mapping

`ds.Schema` 至少包含一张表，每张表至少包含一列。构造时复制并冻结嵌套声明后，`schema.tables` 按声明顺序返回 `tuple[str, ...]`，`schema[table_name]` 返回该表的只读、有序列 Mapping；未知表名使用普通 Mapping 语义抛出 `KeyError`。`schema.create()` 为每张声明创建一个具有相同列顺序、类型与 nullability 的空 `ds.Table`，并按声明顺序返回普通 `dict[str, ds.Table]`；KAT 不公开额外的 `TableSchema` 或 Builder 类型。

Schema 创建的每个 Table 自带对应的单表合同，用户通过 `table.append(**row_values)` 提交一行，不再重复构造同形的列列表 Mapping。Schema 仍允许 D31 已确认的任意非空列名；不能写成 Python keyword literal 的列可以通过 `table.append(**row_mapping)` 提交。

### D38：当前 Workflow 临时物化归入 `ctx.datasource_root`

生产 Workflow 使用 `TemporaryDirectory(dir=ctx.datasource_root)` 创建当前 PACK 私有的临时 workspace，从中派生尚不存在的具体目标路径并把普通 `Path` 传给 Provider；Provider 不接收 Context。所有依赖文件 backend 的 query 必须在 workspace 存活期间完成，eager Table 脱离来源后退出 `with` 清理。最终 Run Output 继续由 Runtime 写入候选 Run 的受管理目录，不由 Provider 或 `ds.write()` 选择位置。

首版不把旧 workspace 当作 cache，也不定义 source identity、parser/version identity、并发锁、失效或回收。`kat.datasource` 是可在普通 Python 与 PACK 测试中使用的纯路径 Toolkit，因此 `ds.write()` 不读取环境变量、隐式当前 Context 或全局 KAT Data Home，也不机械证明传入路径属于 `ctx.datasource_root`；测试可以使用 `tmp_path`。外部输入与已有 Parser 产物也可以从任意获准的只读路径 `ds.open()`。这一边界是 Pack Authoring 合同，不是 Python 文件系统沙箱。

### D39：Python Schema 路径只接受精确值类型

Schema-created Table 的 `append()` 对每个非 null 值使用精确 Python 类型规则：`bool` 只接受 `type(value) is bool`；`int` 只接受非 bool 的精确 `int` 并要求在 Int64 范围；`float` 只接受精确 `float`；`str` 与 `bytes` 只接受各自精确类型；`datetime.datetime` 按 D44 校验时区与范围；`decimal.Decimal` 按 D45 校验精度、scale 与精确性。`None` 只允许出现在 `T | None` 列。

KAT 不把 bool 当 int、不把 int 自动转成 float、不接受 bytearray 代替 bytes，也不调用用户对象的 `__int__`、`__float__`、`__str__` 等转换方法。类型错误在当前 append 期间失败，不等到 Parquet 写入或查询。驱动与高级 Parser 已经形成的类型继续通过 `ds.Table.from_arrow()` 保留；这些 Table 的 append 进一步按 D41 的实际物理类型校验。

### D40：标准 `ds.write()` 固定为平铺的一表一文件

`ds.write(tables, destination=...)` 首版固定为每个 Mapping entry 产生一个 `destination/<name>.parquet`；Table 的全部调用时 Arrow chunks 写入同一文件，零行 Table 也保留一个带 Schema 的文件。函数不为 chunk 生成 part，也不公开分片数、文件命名、压缩算法或 row-group 配置；这些使用 KAT 选择的稳定实现默认值。

需要分片、嵌套目录、特定压缩或其他物理布局的 binary/custom Parser 可以绕过 `ds.write()` 自行产生 Parquet，再通过 `ds.open(..., tables=...)` 接入。因而固定的是标准 Python 落盘函数的低门槛路径，不是所有 Datasource Provider 的存储协议。

### D41：标准 Table 与 Output Query 使用分层类型准入

标准 `ds.Table` 首版只允许经锁定版本 PyArrow、DataFusion 与 Parquet 共同验证的扁平列类型：Boolean、各宽度有符号/无符号整数、Float16/Float32/Float64、Utf8/LargeUtf8/Utf8View、Binary/LargeBinary、`timestamp(ns, tz="UTC")`，以及满足 `0 <= scale <= precision` 的 Decimal128/Decimal256，并允许这些字段 nullable。Table 至少包含一列并继续满足非空唯一列名；每条创建路径还必须检查每个 non-nullable 字段对应的整个 Arrow ChunkedArray 的总 `null_count == 0`。

union、extension、list、struct、map、date、time、duration 与其他未列出的类型不能进入首版标准 Table，Provider 或 SQL 必须在形成标准 Table 前显式转换。`ds.Table(...)`、`ds.Table.from_arrow()` 与 `DataFusionProvider.query()` 都在返回前执行同一 Table admission；不能创建一个 Python 可读但无法交给 DataFusion 或 Parquet Output 的半有效 Table，也不把不支持类型的错误延迟到 Workflow 返回后的发布阶段。

Catalog 是例外的来源查询边界：`ds.open()` 不构造 Table，也不执行 D41 admission，只要求 footer 物理类型可由锁定 DataFusion 扫描。DataFusion Provider 可以在执行计划中读取、展开和转换宽列，但 eager 查询最终结果仍必须整体通过 D41；例如含 list/struct/date/duration 的 Catalog 可以打开，`SELECT *` 在 planning 后、扫描前因 planned result Schema 失败，而显式 `UNNEST`、字段投影或 cast 后可以返回标准 Table。

所有通过准入的 Table 都支持 append。Schema-created Table 使用 D3 的规范物理编码；Arrow/query Table 保留自身物理位宽、nullability、timestamp 与 decimal precision/scale，并按 D49 验证和编码新增行。append 只增加当前 Table 的新 Arrow chunk，不修改或复制已有 Arrow buffers，也不改变产生该 Table 的来源。

已发布 Output 的后续 Output Query 还受现有 JSON Query Result 边界约束。它只准入 Null、Boolean、各宽度整数、有限 Float16/Float32/Float64、Decimal128/Decimal256、Utf8/LargeUtf8/Utf8View 与 `timestamp(ns, tz="UTC")`；Int64/UInt64、Decimal 与 timestamp 分别按既有无损字符串规则输出。Binary 等较宽 Table 类型可以正常融合和发布，但用户必须在 Output Query SQL 中显式投影或 cast 为受支持的 JSON scalar，不能把“可发布 Table”误解为“任意列都可直接返回 JSON”。

### D42：Arrow bridge 使用只读 buffer 移交，不深拷贝整表

`ds.Table.from_arrow()` 不为输入 Arrow Table 无条件深拷贝整个数据。KAT 保持输入 Table 及其 buffers 的强引用；调用后 Provider 可以继续读取原 Table，但不得修改其背后的外部可变 buffer。违反这一只读移交合同后的行为不受保证。DataFusion Provider query 由 KAT 自己形成和持有结果 buffers，自动满足该合同。

bridge 在构造时执行 Arrow 完整结构校验并复用 D41 的共同 Table admission，不把非法 null 延迟到 Parquet publisher。它保证列名、顺序、物理类型与 nullability，不把 Arrow field/schema metadata 纳入标准 Table 合同。标准 `table[name]` 与 `to_rows()` 继续返回和 backing 隔离的 Python 值，不把 Arrow buffer 暴露给普通读取路径。

### D43：`Table.to_arrow()` 提供对称的高级只读桥

Datasource Toolkit 提供 `table.to_arrow() -> pyarrow.Table`，与 D26 的 `ds.Table.from_arrow()` 对称。它取得调用时快照并共享 Table 已有的只读 buffers，不复制历史整表；调用方不得修改返回 Arrow Table 背后的 buffer。普通作者仍使用 `table[name]` 与 `to_rows()`，这一 bridge 不给 Table 增加 DataFrame 或 compute 方法。

该桥使 eager `DataFusionProvider.query()` 结果可以显式进入既有 `ctx.from_arrow()` DataFrame 路径，例如继续使用只接收 DataFusion `Expr` 的 `ctx.convert_clock()`；KAT 不因此注册时钟 SQL UDF，也不改变 `ctx.from_arrow()`、`ctx.convert_clock()` 或 DataFrame Output 的迁移语义。需要重新形成标准 Table 时，高级调用方可以完整 collect Arrow 后再使用 `ds.Table.from_arrow()`。

### D44：Python `datetime` 固定表示 UTC nanosecond timestamp

Datasource Schema 首版接受 `datetime.datetime` 与 `datetime.datetime | None`。非 null 值必须是精确、带有效 UTC offset 的 Python datetime；KAT 在写入前把它换算到 UTC，规范物理类型固定为 Arrow `timestamp(ns, tz="UTC")`。naive datetime 直接失败，不读取本机时区，也不把缺失 offset 猜成 UTC。

Catalog 可以保留锁定 DataFusion 可扫描的其他 date/time/timestamp 物理类型；`ds.open()` 不把它们解释为 Python datetime，也不扫描列值。SQL 最终结果若要形成标准 timestamp Table 列，必须显式转换为 `timestamp(ns, tz="UTC")`，普通 `CAST(... AS TIMESTAMP)` 若仍产生无时区 timestamp 不满足 D41。Table 的 Python 读取使用既有 nanosecond formatter 直接形成 `kat.WallClockTimestamp`，不调用 PyArrow 的 timezone-to-`datetime` 转换，从而保留纳秒并避免依赖主机 timezone database。该语义与 DataFusion scalar 和 Output Query 的绝对 UTC 时间边界一致。

### D45：Python `Decimal` 使用固定 `ds.write()` 编码并兼容已有 Decimal

Datasource Schema 首版接受 `decimal.Decimal` 与 `decimal.Decimal | None`。`Table.append()` 的规范物理类型固定为 Arrow `decimal128(38, 18)`：只接受有限的精确 Decimal；可以补零，或仅删除不改变数值的尾随零，以无舍入方式精确 rescale 到 18 位小数。rescale 后最多保留 20 位整数与 18 位小数；任何需要四舍五入、NaN、Infinity 或超出 precision/range 的值立即失败，不从首批数据推断 scale。Table 的 Python 读取保持 `decimal.Decimal`。

Catalog 直接保留锁定 DataFusion 可扫描的 Decimal 物理表示，不把它映射为 Python Schema，也不重写 precision/scale；DataFusion query 最终结果中的 Decimal 仍必须满足 D41 的 Decimal128/256 与 `0 <= scale <= precision`。首版不增加 `ds.Decimal(precision, scale)`、`typing.Annotated` metadata 或第二种 Schema DSL；若 `ds.write()` 的固定编码不能覆盖后续真实 Python Parser，再从同一个 Python Schema 模型增量扩展。

### D46：PR #229 用三个独立 Example PACK 验证三类 Provider

第二个交付切片不再只包含 PostgreSQL，而是分别提供远端数据库、Python 文件解析和外部二进制解析三个可复制的 Example PACK。三者只在 eager `ds.Table` 结果处收敛，不新增 Provider 基类、Protocol、Executor 或统一生命周期：PostgreSQL Provider 直接执行远端 SQL；Ftrace 文本 Provider 通过 `ds.Schema`、`ds.write()` 和 `ds.open()` 形成多表 Parquet catalog，并要求调用方显式提供 capture 的 Clock domain；Trace Streamer Provider 调用外部程序生成 SQLite，并用来源自己的只读 SQL 查询后按显式 Python Schema 形成 Table。

两个文件 Provider 的 `decode()` 每次只重建 Workflow 在 `ctx.datasource_root` 临时 workspace 中、明确交给该 Provider 独占的物化目标；所有 query 完成并形成 eager Table 后清理 workspace。decode fail-closed，只在完整成功后提交 backend，失败保持未准备且可重试。首版不复用跨 Workflow 或跨进程旧产物，不增加 cache key、manifest、原子替换、锁或恢复协议。Ftrace 大样本、Htrace、Trace Streamer 可执行文件、生成数据库和 sidecar 都是外部测试输入，不进入仓库。

### D47：Toolkit 不隐藏延长 Table 生命周期

`ds.Schema.create()` 只返回新建 Table Mapping，不由 Schema 保存；`ds.write()` 只在同步调用期间持有输入快照；`ds.Catalog` 只持有 Parquet relation 路径与轻量 footer Schema 描述，不引用生成这些文件的内存 Table；一次 DataFusion query 的短命 Session 在完成后不额外持有输入 Table。DataFusion Provider 自身会显式强引用构造时的 Table/Catalog，直到 Provider 也离开作用域。调用方保存在其他变量、`table.to_arrow()` 结果或可能共享输入 buffers 查询结果中的 Table 同样遵守普通强引用；KAT 不承诺在这些引用存在时释放内存，也不承诺 allocator 立即把已空闲地址空间归还操作系统。

### D48：新 Datasource 融合由具体 DataFusion Provider 承接

KAT Datasource Toolkit 提供普通具体类 `ds.DataFusionProvider(tables=None, catalog=None)`。Workflow 或 PACK 显式构造并调用它：`tables` 提供具名内存 Table，`catalog` 提供 `ds.open()` 已校验的多张磁盘 Parquet relation，至少提供一个且二者可以混合；查询 eager 返回标准 `ds.Table`。它不是 Datasource Provider 基类、Runtime facade、全局 registry 或 Binding，也不发现、构造或调用来源 Provider。

远端数据库仍由对应 Datasource Provider 显式执行来源 SQL；Workflow 只把已经得到的 Table 交给 DataFusion Provider。DataFusion Provider 不做 SQL 拆分、透明下推、来源物化、Parquet 落盘或 Run Output 发布。Catalog 自身不创建 Session 或提供 query；`ctx.sql()` 仅保留旧 Dataset grant 的兼容执行面，待旧 Dataset 迁移后删除。ADR-0066、ADR-0067、ADR-0068 记录该边界，并取代 D8、D11、D22、D23、D25 与 D35 中的旧查询入口表述。

### D49：物理 Table append 使用严格 Python 值族和 Arrow 编码

`Table.from_arrow()` 与 `DataFusionProvider.query()` 形成的 Table 按每列实际 Arrow 物理类型接受 Python 值：Boolean 只接受精确 `bool`；signed/unsigned integer 只接受排除 bool 的精确 `int` 并按实际位宽检查范围；Float16/32/64 只接受精确 `float`，允许目标宽度的正常 IEEE 精度舍入，但有限输入若编码后溢出为非有限值则失败；Utf8、LargeUtf8 与 Utf8View 只接受精确 `str`；Binary 与 LargeBinary 只接受精确 `bytes`；nullable field 额外接受 `None`，non-nullable field 拒绝。

`timestamp(ns, tz="UTC")` 接受精确的 aware `datetime.datetime` 或 `kat.WallClockTimestamp`，由 KAT 自己规范成 epoch nanoseconds，拒绝 naive datetime、str 与裸 int。Decimal128/Decimal256 只接受精确且有限的 `decimal.Decimal`，按实际 precision/scale 只允许无舍入 rescale，并要求 Parquet 可写的 `0 <= scale <= precision`；int、float、str、NaN、Infinity 或越界值失败。显式传入的 float NaN/Infinity 可以存在于标准 Table，但仍受 D41 的 Output Query JSON 限制。

KAT 不直接采用 PyArrow 过宽的 scalar coercion。append 先规范整行，所有列成功后才一次性加入内部待编码缓冲；形成任何读取、查询、写入或 Output 快照时，把 pending rows 编码成新的 Arrow chunk，并以不提升类型的方式组合 chunk。实现不得 cast 或 combine 历史 chunk，因此旧 Arrow snapshot、此前查询结果和已有 buffers 不随之后 append 改变。

### D50：DataFusion query 参数使用显式、无歧义的 Scalar 集合

`DataFusionProvider.query(sql, params=...)` 的参数 Mapping key 遵守 D24，value 只接受：精确 bool；排除 bool 且在 signed Int64 范围内的精确 int；有限的精确 float；精确 str；精确 bytes；带有效 UTC offset 的精确 `datetime.datetime`；`kat.WallClockTimestamp`；精确且有限的 `decimal.Decimal`；以及 `kat.Duration`。KAT 在交给 DataFusion 前分别规范为 Boolean、Int64、Float64、Utf8、Binary、`timestamp(ns, tz="UTC")`、Decimal128/Decimal256 或 Int64 nanoseconds，不接受 DataFusion 原生的跨类型隐式转换。

datetime 按绝对 instant 规范到 UTC 纳秒，naive datetime 失败；WallClockTimestamp 保留九位纳秒。Decimal 按值无舍入地选择最小可容纳且满足 `0 <= scale <= precision` 的类型，precision 不超过 Decimal256 的 76；Duration 必须落在 signed Int64 nanoseconds 范围。`None` 因缺少目标类型而拒绝；NaN/Infinity、bytearray、memoryview、date、timedelta、容器、任意 `pyarrow.Scalar` 及其他对象也拒绝。参数错误发生在 SQL planning 前，只影响当前调用。

## 7. 迁移范围

### 7.1 公共接口迁移

本设计对尚未合并的 PR #228 进行直接替换，不为该 PR 新增的 facade 接口保留兼容层：

- 删除顶层 `kat.Provider`、`kat.SourceExecutor`、`kat.ParquetSource` 与 operation-bound `kat.Table`；
- 删除 `ctx.provider()`、Provider query 自动命名、自动注册、提前写入候选 Output 和 operation catalog；
- 新增唯一 Toolkit namespace `kat.datasource`，只从这里公开 D28 列出的能力，包括具体 `DataFusionProvider`；
- `DataFusionProvider(tables=None, catalog=None).query(sql, *, params=None) -> ds.Table` 成为新 Datasource 唯一本地 SQL 接口；
- `ds.Catalog` 删除 `query()`，只作为 `ds.open()` 返回并传给 DataFusion Provider 的具名 Parquet relation 集合；
- `ctx.sql(sql, **params) -> DataFrame` 保持原签名、scalar 参数与惰性返回类型，不接收新 Datasource Table 或 Catalog，只查询旧 Dataset grant；
- 暂时保留 `ctx.from_arrow(pyarrow.Table) -> DataFrame`、`ctx.convert_clock(...)` 与 DataFrame Output，使普通 Arrow authoring path 可以独立迁移；
- Workflow Output 在迁移期接受 `ds.Table`、旧 DataFrame，以及二者组成的非空普通 `dict`。

### 7.2 Runtime 迁移

以下已有边界保持不变：

- `ctx.datasource_root` 及 CLI 到 Workflow Runtime 的 PACK 范围路径注入与校验；
- 旧 Dataset、`required_tables`、Table Grant、Test Dataset 与 `kat import`；
- Run Manifest、Runtime Response、最终 Output Parquet 与 `kat query --run`；
- `ctx.from_arrow()`、时钟转换与旧 DataFrame Output 的受管理执行生命周期。

以下 PR #228 实现由新模型替换：

- Provider executor 登记与 operation-level close 编排；
- query result name、SQL hash 缺省名与名称保留；
- Provider query scratch、partial、提前 Output Parquet 与 backing 复用；
- operation-local Provider relation catalog；
- Provider query 失败后的 Context poison；
- Table 与 operation token、backing path 和 Execution Lease 的绑定。

新的 DataFusion Provider 每次 `query()` 创建独立 Session，只注册构造时显式提供的内存 Table 与 Catalog relation，执行完成后形成与 Session 脱离的 eager `ds.Table`。它不经过 Runtime，也不取得 Dataset grant。Workflow 调用前仍执行既有 Required-table grant 检查，兼容 `ctx.sql(sql, **params) -> DataFrame` 继续通过原受管理执行面只访问获授权的旧 Source table，不能把 Dataset 授权路径公开给公共 Provider。

### 7.3 仓库内代码迁移

| 当前区域 | 目标状态 |
|---|---|
| `kat/platform/workflow/api/_datasource.py` | 用 `kat.datasource` 的 Schema、Table、Catalog、DataFusionProvider 与构造函数替换 Provider facade 模型 |
| `kat/platform/workflow/api/_workflow.py` | 删除 `provider()`；原样保留旧 Dataset `sql(sql, **params) -> DataFrame`、`from_arrow()`、`convert_clock()` 与 `datasource_root` |
| `kat/platform/workflow/runtime/datasource.py` | 删除 Provider operation 状态机；可复用本地查询机制下沉到 DataFusionProvider，Output 仍归 Runtime |
| `kat/platform/workflow/runtime/execution.py` | 保留旧 Dataset grant/Lease 查询；不再承担新 Datasource Fusion Session |
| `kat/platform/workflow/runtime/outputs.py` | eager Table 只在 Workflow 返回后写入最终 Output；继续支持迁移期 DataFrame 与混合 dict |
| `examples/packs/local-parquet-fusion` | Provider 移到顶层 `datasources/`，通过 `ds.open()` 与 `DataFusionProvider(catalog=..., tables=...)` 完成本地融合 |
| 旧 Dataset Bundled PACK | 继续通过兼容 `ctx.sql()` 使用已授权 Source table，不迁入新 Provider |
| PR #228 Provider 测试 | 删除 facade、自动注册、name/backing、poison 与 executor close 合同，改测 Toolkit、call-local Fusion 与最终 Output |
| PR #229 Provider 案例分支 | 分别提交 PostgreSQL、Python Ftrace 文本和 Trace Streamer SQLite 三个独立 Example PACK，不把旧 `SourceExecutor` 兼容层或 Rust Data Import 带入新设计 |

Issue #226、PR #228 描述与相关 ADR 必须先同步为本文模型，再实现代码；交付说明不能继续承诺 Provider facade、自动注册、`Catalog.query()` 或以 `ctx.sql()` 承担新 Datasource 融合。

## 8. 两阶段交付

| 切片 | 交付内容 | 明确不包含 | 完成证明 |
|---|---|---|---|
| PR #228：Toolkit 与本地融合 | `kat.datasource`；Schema、Table、Arrow bridge、`write()`、`open()`、只读 Catalog、DataFusionProvider；顶层 PACK `datasources/`；内存/Parquet/混合 eager 查询；Table Output；原样保留旧 Dataset `ctx.sql(sql, **params) -> DataFrame`；本地 Parquet Provider 与融合案例 | PostgreSQL Provider、ADBC 依赖、远端服务测试；旧 Dataset/Import 删除；Hitrace/Ftrace 迁移 | Python 合同测试、Runtime 子进程测试、External PACK 用户链、Bundled PACK 回归、Linux/Windows Full CI |
| PR #229：三类 Provider 案例 | 三个独立 Example PACK：PostgreSQL 远端 SQL；Python Ftrace 文本流式解析到多表 Parquet；Trace Streamer 外部二进制解析到 SQLite 后按显式结果 Schema 查询；各自 README、Workflow 与 PACK pytest | 通用 Provider/Parser 框架；数据库 registry；透明 federation；Rust Data Import 迁移；跨进程物化缓存；提交外部样本、二进制或生成数据库 | PACK pytest 公共 Interface；PostgreSQL 现有合同；真实 Ftrace 样本 44,344 条事件；真实 Trace Streamer 4.3.7 生成 SQLite 并验证 `native_hook` 聚合；本地 Workflow Output |

PR #228 必须独立形成可运行的本地纵向切片。PR #229 不重新设计核心接口，只用三种不同来源证明普通 PACK Provider 能复用同一 Schema、Table、Output 与 Fusion 边界。

## 9. 明确不做

- 不建立 Provider 基类、Protocol、facade、factory、registry、decorator、entry point、Binding 或平台发现机制；
- 不自动扫描或预加载 PACK `datasources/`；
- 不实现透明跨源 SQL、SQL 拆分、自动下推、Federation planner、远端 TableProvider 或成本优化；
- 不统一远端数据库的 SQL 方言、参数、事务、驱动、连接池与资源协议；
- 不提供通用 DB-API、ADBC executor 或数据库 helper；
- 不建立 Parser 基类或 Parser registry，也不规定 Provider 必须暴露统一方法集合；
- 不要求 binary/custom Parser 使用 `ds.write()`，也不把数据库型解析产物整体转换为 Parquet；
- 不提供 lazy 或 streaming 标准 Table；新 Datasource 唯一本地 SQL 入口 `DataFusionProvider.query()` eager 返回 Table；
- 不把 `ds.Table` 扩张成 DataFrame；除完整行 `append()` 外，不增加 update、delete、列结构 mutation、filter、select、group-by 或表达式 API；
- 不在首版扩张 D3、D44、D45 的七种 Python Schema 逻辑类型；高级类型只通过 Arrow bridge 或查询结果保留；
- 不在本切片修改或删除 `ctx.sql(sql, **params) -> DataFrame`、`ctx.from_arrow()`、DataFrame Output、旧 Dataset、`required_tables`、Test Dataset 或 `kat import`；
- 不迁移或复用现有 Rust Hitrace/Ftrace/native Parser，也不改变 Rust Datasource 与 `kat import` 的现有合同；PR #229 的 Ftrace 与 Trace Streamer 只作为 PACK Python Provider 案例；
- 不提交真实 Ftrace/Htrace、大型 Trace Streamer 可执行文件、其依赖 DLL、生成 SQLite 或 sidecar；
- 不建立 Datasource materialization 的 manifest、版本、迁移、锁、恢复、原子发布或平台清理框架；
- 不增加跨 PACK import、PACK 依赖系统或逐 PACK Python 环境；
- 不实现自动重试、备用 Provider、跨 Provider 并行或查询缓存；
- 不改变已发布 Run Output、Run Manifest 或 `kat query --run` 的现有持久语义；
- PostgreSQL 测试只依赖可访问的真实服务；镜像、容器创建、等待、销毁与复用不属于交付范围。

## 10. 验收与验证

### 10.1 PR #228

| 边界 | 必须验证 |
|---|---|
| 公共 namespace | `from kat import datasource as ds` 可用；公开 D28 的精确表面；旧 `kat.Provider`、`SourceExecutor`、`ParquetSource`、`kat.Table` 与 `ctx.provider()` 不再存在 |
| Schema | 输入声明被复制冻结；至少一表一列；表名与列名规则；声明顺序；七种逻辑类型与 `T \| None`；原始 Mapping 后续修改不影响 Schema |
| Table 构造与追加 | `Schema.create()` 与 `ds.Table(single_table_schema)` 创建空表；完整单行原子 append；缺列、多列、类型、范围与 nullability 严格失败且不留半行；不公开 `ds.table()`、Builder 或批量 columns/rows constructor |
| Arrow bridge | 保留名称、顺序、物理类型与 nullability；共享 buffer 的只读所有权；所有 non-nullable ChunkedArray 的总 null count；拒绝空名、重名、嵌套与未准入类型；metadata 不构成合同 |
| Table 类型 | Schema、from_arrow、Source query 与 DataFusion query 都产生同一种可追加 Table；D41 的精确物理类型矩阵；Arrow/query Table append 保留物理 Schema；UTC ns 读取为 `WallClockTimestamp` 且保留九位精度；Decimal 读取为 `Decimal` |
| 物理 append | D49 每种 Arrow 类型的精确 Python 值族；整数范围；窄 float 正常舍入与有限溢出失败；UTC ns 两种输入；Decimal exact rescale；nullable；拒绝 PyArrow 隐式 coercion；整行失败不留半行 |
| append 快照 | pending rows 只形成新 chunk；不 cast/combine 历史 chunk；append 后旧 `to_arrow()`、查询、写入和 Output 快照的值与 buffer 地址保持不变 |
| datetime 合同 | aware datetime 按绝对 instant 规范到 UTC；naive、无有效 offset 与 signed Int64 ns 越界立即失败；Schema-created Table 固定 `timestamp(ns, tz="UTC")`；Catalog 可保留更宽 date/time，查询结果必须显式规范到 D41 |
| Decimal 合同 | Schema-created Table 固定 `decimal128(38,18)`；补零或只删尾随零的 exact rescale 成功；需舍入、NaN、Infinity 与 precision/range 越界失败；Catalog 保留 DataFusion 可扫描的来源表示，from_arrow/查询结果执行 D41 |
| `ds.write()` | 多表 Mapping、任意 entry 顺序、零行表、一表一文件；调用时快照；destination 已存在失败；验证、写入或 close 失败尽力清理整个目标目录；无公共 Writer 对象 |
| `ds.open(root=...)` | 不接收 Schema；只发现直属 Parquet 文件；忽略其他扩展名；至少发现一张 relation；表名取合法文件 stem；自洽子集可以打开且不承诺预期集合完整 |
| `ds.open(tables=...)` | Mapping 非空且每个绑定路径必须存在；文件与递归 parts 目录；稳定 part 顺序；每个 parts 目录至少一个 part；忽略非 Parquet 文件；缺少任一预期 relation 失败；不推导 Hive partition 列 |
| Parquet 物理 Schema | footer 类型必须由锁定 DataFusion 扫描但可以宽于 D41；至少覆盖 list/struct/date/duration；保留 physical nullability；所有 parts 的列名、顺序、类型与 nullability 一致；忽略 Arrow metadata；损坏 footer 在 open 时失败 |
| 宽 Catalog 查询 | 宽来源的 `SELECT *` 在 planning 后、扫描前因 planned result Schema 失败；list 展开/聚合、struct 字段投影、date/timestamp 与 duration 显式 cast 后得到 D41 Table；Catalog 不为宽列构造中间 Table |
| Catalog | open 只读取 metadata、不扫描全部数据行、不创建 Session；root/tables 发现、路径与物理 Schema 校验；`catalog.tables` 为名称排序 tuple；不公开 schema/Relation/pyarrow.Schema；没有 query/close；源路径删除或替换只影响后续 DataFusion Provider 扫描 |
| Catalog inspection | `DESCRIBE <relation>` 返回列名、DataFusion 类型文本与 nullability 的 D41 Table；`SHOW TABLES` 可以执行但包含 information schema；无需 Catalog 列 Schema API |
| DataFusion Provider | 内存多 Table、Catalog 多 Parquet 与二者混合；至少一种输入；名称冲突前置失败；每次 query 使用隔离 Session；planned result D41 检查发生在扫描前；collect 后检查实际 nullability；只扫描 SQL 引用的 Parquet；失败后同一 Provider 可重试 |
| Provider 复用 | 构造后 relation 集合不可变；修改原 Mapping 不生效；两次查询间 append 只在后一次快照可见；Provider 存活时输入 Table/Catalog 保持强引用；构造新 Provider 才能换 relation |
| Table 脱离 | DataFusion Provider query 返回后，释放 Provider/Catalog 或删除源文件不影响既有结果 Table 的 Python 读取、后续 Fusion 或 Output |
| 文件 Provider 状态 | decode 入口清空 ready；独占旧目标删除失败、parse/write/open/backend 任一步失败均保持未准备并尽力清理；成功才一次提交；失败后可重试；同一实例/路径并发 decode/query 不受支持 |
| 兼容 `ctx.sql()` | 保持 `ctx.sql(sql, **params) -> DataFrame`；旧 Dataset required_tables、Table Grant、Execution Lease、缺表、scalar 参数与惰性执行现有行为回归；不能接收或隐式访问新 Datasource Table/Catalog |
| `ctx.datasource_root` | 生产值位于当前 KAT Data Home 的 PACK 私有范围且只读、受 Lease 约束；Workflow 用 `TemporaryDirectory(dir=...)` 创建当前执行 workspace 并只传派生 Path；退出后 eager Table 仍可用；旧 workspace 不复用；同一 `kat test` 的测试根可共享但每次 `kat_run` 的 workspace 独立，不同 test 根彼此隔离且不写生产 Data Home |
| SQL 准入 | DataFusion Provider 接受 SELECT/WITH/VALUES/DESCRIBE/EXPLAIN 与结果满足 D41 的只读 SHOW；`SHOW FUNCTIONS` 因嵌套列失败；多语句、DDL、DML、COPY 与 Session mutation 被拒绝 |
| SQL 参数 | D50 精确值族；bytes、UTC ns datetime/WallClockTimestamp、Decimal128/256、Duration；参数 Mapping 调用时快照；拒绝 None、naive 时间、非有限 float、PyArrow scalar 与隐式 coercion |
| Output | 单 Table 使用 `main`；非空普通 dict 显式命名；支持 Table/DataFrame 混合；中间 Table 不产生 Output；来源 SQL 不重复执行；零行 Table 保留 Schema |
| Output Query | D41 的 wider Table 可以发布；JSON Query Result 只接受其更窄标量集合；Binary 等列必须先在 SQL 中显式投影；非有限 float 失败 |
| DataFrame 兼容 | `ctx.from_arrow()`、`convert_clock()`、旧 Dataset `ctx.sql(sql, **params) -> DataFrame` 与 DataFrame Output 的现有合同和测试继续通过；新 Datasource query 只由 DataFusion Provider eager 返回 Table |
| PACK namespace | `kat.pack.datasources.*` 的普通 import、namespace package 与可选 `__init__.py` 行为；不扫描、不预加载；副作用 Workflow 注册仍被入口规则拒绝 |
| Bundled PACK | thread CPU time 的 Table Output 与 Output Query 保持可用；critical-path 迁移后全部 PACK 测试通过 |
| 本地纵向案例 | External PACK 完成 inspect、test、run 与 Output Query；验证纯内存 Table、纯 Catalog Parquet 和二者混合的 DataFusion Provider 查询 |
| 平台回归 | Linux 与 Windows Full CI 都通过，不以单平台结果代替 |

建议至少记录以下实际证据：

```text
python -I -B -m unittest discover -s kat/platform/workflow/tests -p "test_*.py"
cargo test --workspace --features kat-datasource/protobuf-source-contract-fixture --locked
cargo test --locked -p kat-cli --test trace_streamer_demo trace_streamer_demo_runs_the_full_user_loop -- --ignored --exact
```

真实 Host 条件下还应执行两个 Bundled PACK 的 `kat test`，以及本地案例 README 中的 inspect、test、run、query 命令。PR 描述必须记录实际命令、平台与结果，不能只列计划。

### 10.2 PR #229：三类 Provider 案例

| 边界 | 必须验证 |
|---|---|
| Provider 形态 | `PostgreSQLProvider` 是 PACK 普通类，不继承 KAT 类型、不接收 Context、不调用 `ctx.sql()` |
| Source query | SQL 与位置参数交给 PostgreSQL；同库 Join、Filter、Aggregate 在远端完成 |
| Table bridge | ADBC 结果完整读取后经 `ds.Table.from_arrow()` 返回 eager Table；零行仍保留结构；Provider 在 bridge 前把有绝对时间语义的 timestamp 规范为 D41 的 `timestamp(ns, tz="UTC")`，或让来源 SQL 直接返回该类型；decimal、整数、浮点、布尔、文本与 null 按 D41 保真 |
| 单源 Output | Workflow 可直接返回 PostgreSQL Table，不执行第二次远端 SQL |
| 多库融合 | 同一 service 下依次查询两个不同 Database，并把各自 Table 与本地 Parquet Catalog 显式交给同一个 DataFusion Provider |
| 资源生命周期 | Table 返回前 cursor、reader、transaction 与 query-local connection 已关闭；若 Provider 选择复用长期资源，则由普通显式 context manager 管理 |
| 只读与参数 | 服务端只读事务成立；写权限测试角色仍无法执行 DML、DDL 与 COPY；参数绑定不做 SQL 文本替换 |
| 错误语义 | 连接、认证、SQL、读取与关闭失败不泄露 service、Database、密码或连接字符串；Workflow 可按普通 Python 规则处理 Provider 错误 |
| 测试归属 | Provider、Workflow 与融合案例只由 example PACK 的 pytest 维护并使用生产 Provider；Rust 只保留通用 CLI/Runtime 合同测试，不增加 PostgreSQL PACK 专用测试目标 |
| 用户链 | `kat test` 执行 PACK pytest，并通过 `kat_run` 覆盖单源 Output 与多源融合，不使用 test-only Provider |
| 平台 | 需要真实服务证据时，Linux、Windows 分别用同一套 PACK pytest 执行；测试未执行或被 skip 不构成支持证据 |
| Example PACK 分布 | PostgreSQL、Ftrace 文本与 Trace Streamer SQLite 分别拥有独立 `pack.toml`、`datasources/`、`workflows/`、`tests/` 与 README；各自前置条件互不污染 |
| Ftrace Schema | 用基础 Python 类型声明 `capture` 与 `events` 两张表；调用方显式提供本次 capture 的 `clock_domain`，事件同时携带 `clock_domain`、1 GHz `clock_value` 与源文件顺序 `event_index`，不把 Clock value 命名或声明为 UTC timestamp |
| Ftrace decode | 当前 Workflow 临时 workspace；单遍按行解析并 append 到 `Schema.create()` 产生的 Table；坏行带行号失败；解析完成后一次 `ds.write(tables, ...)`，成功后以 `tables={"capture": ..., "events": ...}` 显式打开完整预期集合；fail-closed 且不复用旧目录 |
| Ftrace query/Output | `query()` 委托只持有 Catalog 的 DataFusion Provider，返回可重复读取的 Table；小 fixture 覆盖公共合同，真实样本验证 44,344 条事件、4 个 CPU 和 Workflow Output |
| Trace Streamer decode | 当前 Workflow 临时 workspace；使用参数数组而非 shell 执行 `trace_streamer <trace> -e <output.db>`；本次退出成功、DB 存在且 SQLite 完整性检查通过后才 ready；fail-closed，失败残留清理且不伪装为成功 |
| Trace Streamer query | 标准库 `sqlite3` 以 `mode=ro`、`query_only` 与只读 authorizer 执行来源 SELECT，显式拒绝 `ATTACH` 等外部副作用；调用方用基础 Python 类型声明结果 Schema；完整读取并关闭数据库后创建 `ds.Table(result_schema)` 并逐行 append；不整体转 Parquet，不增加 ADBC SQLite 依赖 |
| Trace Streamer 真实样本 | 用外部 Trace Streamer 4.3.7 与 Htrace 生成 SQLite，验证 `native_hook` 聚合结果；外部 exe、DLL、Htrace、DB 与 sidecar 不提交 |
| 测试归属 | 三个案例的 Provider、Workflow、真实来源与错误行为都由各自 PACK pytest 维护；不增加案例专用 Rust 测试或平台 pytest 包装 |

建议记录真实环境命令：

```text
kat test --pack-dir ./examples/packs/postgresql-parquet-fusion
kat test --pack-dir ./examples/packs/ftrace-text-provider
kat test --pack-dir ./examples/packs/trace-streamer-sqlite-provider
```

需要声明 Linux 与 Windows 的真实 PostgreSQL 支持时，对应 PACK pytest 必须在两端各执行一次，并在 PR 中记录实际通过结果、Workflow Host wheel、服务配置方式，以及秘密未进入 pytest 公开错误与 Runtime Response 的检查结果。Ftrace 与 Trace Streamer 的本地支持证据必须记录实际外部样本、二进制版本和未 skip 的 PACK pytest 结果。平台通用的 Operation log、Run Manifest 与 KAT Data Home 边界继续由既有 CLI/Runtime 测试负责，不在 PACK 中复制。
