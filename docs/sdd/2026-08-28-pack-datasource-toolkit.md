---
status: accepted
---

# PACK Datasource Toolkit 简化设计

## 1. 背景

PR #228 把来源扩展拆成 PACK `SourceExecutor`、`ctx.provider()` 与 KAT `Provider` facade。虽然这条链路集中处理了来源结果落盘和注册，但同一个 Provider 概念同时指向 PACK 来源实现与 KAT facade，PACK 作者还需要理解 executor、facade、Arrow stream、scratch 和 Runtime 生命周期，公开模型超过了来源接入本身所需的复杂度。

本文取代 `2026-08-26-pack-datasource.md` 与 ADR-0062 中的 Provider 所有权和作者接口决定，并通过 ADR-0063、ADR-0064 记录新的稳定边界。旧设计中仍然有效的多数据源本地融合、Run Output 和迁移期兼容原则在本文中重新表述。

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
  │    ├─ Python Parser → ds.write() → Parquet Catalog
  │    └─ 已有 Parquet ─────────────→ ds.open() → Parquet Catalog
  │                                                └─ query() → ds.Table
  │
  ├─ 数据库或远端服务
  │    └─ Provider 原生 query → Python rows 或 Arrow → ds.Table
  │
  └─ ds.Table
       ├─ 由 Python 直接读取
       ├─ 直接作为单源 Workflow Output
       └─ 显式传给 ctx.sql(..., tables=...) 做多源融合
                                      └─ ds.Table
                                           └─ Workflow 返回后由 Runtime 发布
```

`kat.pack.datasources.*` 保存 PACK 拥有的 Provider、Schema 与解析代码；`kat.datasource` 是 KAT 提供的普通数据面 Toolkit。KAT 不发现、注册、构造或包装 Provider。

Provider 负责数据从哪里来、如何解析、如何执行来源内查询，以及是否维护可复用物化。Provider 的方法集合、SQL 方言和参数完全属于来源。`ctx.sql()` 只处理已经形成的本地 `ds.Table` 与迁移期 Dataset grant，不发现 Provider，也不隐式执行来源查询。

文件 Provider 可以把多表解析结果写入 `ctx.datasource_root` 下的 PACK 私有目录，再用 `ds.open()` 建立只读 Catalog。数据库 Provider 不需要整体转储数据库，只把某次查询结果通过 `ds.table()` 或 `ds.from_arrow()` 形成 eager `ds.Table`。Workflow 返回后，Runtime 才把被选中的 Table 写成最终 Run Output；仅用于 Python 计算或融合的中间 Table 不产生 Output 文件。

### 3.2 对象关系

| 对象 | 所有者 | 负责 | 不负责 |
|---|---|---|---|
| Datasource Provider | PACK | 来源定位、解析、来源 SQL、来源资源和可复用物化 | KAT 注册、跨源 planning、Run Output 发布 |
| `ds.Schema` | PACK 声明、KAT 校验 | 多表逻辑结果合同 | Parser 算法、Schema 推断、演进 |
| Writer | KAT Toolkit | Python 列 batch 校验与一表一文件 Parquet 写入 | artifact key、缓存、查询 |
| `ds.Catalog` | KAT Toolkit | 对稳定 Parquet 路径执行本地只读 SQL | 文件所有权、快照、远端 SQL |
| `ds.Table` | KAT Toolkit | eager、可重复读取的标准单表结果 | relation name、Provider identity、DataFrame API |
| Workflow Context | KAT Runtime | PACK 存储根、call-local Fusion query、迁移期旧能力 | Provider factory、Provider 生命周期、隐式 catalog |
| Workflow | PACK | 选择 Provider、调用顺序、融合步骤和最终 Output | 透明 federation、Runtime 发布实现 |

同一个 Workflow 可以顺序查询同一远端服务中的多个 Database。是否复用连接池、如何切库、SQL 参数和只读事务都由该 PACK 的 Provider 定义；KAT 只在各次查询已经形成 Table 后参与本地融合。

## 4. 公共 API 总览

以下签名描述首版规范表面。路径只接受 `pathlib.Path`；输入 Mapping 在调用开始时快照。`PythonColumnType` 表示 `bool`、`int`、`float`、`str`、`bytes`、UTC-aware `datetime.datetime` 或 `decimal.Decimal`，也可以写成 `T | None`。

```python
from kat import datasource as ds
```

### 4.1 Schema 与 Writer

```python
schema = ds.Schema(
    tables: Mapping[str, Mapping[str, PythonColumnType]],
)

schema.tables -> tuple[str, ...]
schema[table_name] -> Mapping[str, PythonColumnType]

with ds.write(
    schema,
    *,
    destination: pathlib.Path,
) as writer:
    writer.write(
        table_name: str,
        /,
        **columns: list[object | None],
    ) -> None
```

Schema 至少包含一张表，每张表至少一列；声明在构造时复制并冻结。Writer 要求 destination 不存在，固定产生平铺的 `destination/<table>.parquet`。成功退出后需要另行调用 `ds.open()`。

### 4.2 Catalog

```python
catalog = ds.open(
    schema: ds.Schema,
    *,
    root: pathlib.Path | None = None,
    tables: Mapping[str, pathlib.Path] | None = None,
) -> ds.Catalog

catalog.query(
    sql: str,
    *,
    params: Mapping[str, Scalar] | None = None,
) -> ds.Table
```

`root` 与 `tables` 必须且只能提供一个。Catalog 是对源 Parquet 路径的只读 live view；查询结果是与 Catalog、Session 和源文件脱离的 eager Table。Catalog 与 `ctx.sql()` 使用同一套 DataFusion 单语句只读准入与 scalar 参数规则。

### 4.3 Table 构造与读取

```python
table = ds.table(
    *,
    schema: Mapping[str, PythonColumnType],
    columns: Mapping[str, list[object | None]] | None = None,
    rows: Sequence[Sequence[object | None]] | None = None,
) -> ds.Table

table = ds.from_arrow(arrow_table: pyarrow.Table) -> ds.Table
arrow_table = ds.to_arrow(table: ds.Table) -> pyarrow.Table

len(table) -> int
table.columns -> tuple[str, ...]
table[column_name] -> tuple[object | None, ...]
table.to_rows() -> list[dict[str, object | None]]
```

`columns` 与 `rows` 必须且只能提供一个。Python 值使用精确类型校验，不做隐式转换或推断。Arrow bridge 只准入可被锁定版本 PyArrow、DataFusion 与 Parquet 共同处理的扁平 Table 类型；两端共享只读 buffer，不深拷贝整表。Output Query 的 JSON 结果类型是 D41 中单独定义的更窄集合。

Table 至少包含一列，列名必须是非空且唯一的字符串。Table 不提供 mutation、filter、select、group-by、表达式或 DataFrame API。

### 4.4 Fusion query 与 Output

```python
result = ctx.sql(
    sql: str,
    *,
    tables: Mapping[str, ds.Table] | None = None,
    params: Mapping[str, Scalar] | None = None,
) -> ds.Table
```

每次调用创建独立短命 DataFusion Session，注册显式 Table 与迁移期 Dataset grant，完整执行后返回 eager Table，不保留跨调用 catalog 状态。`Scalar` 继续使用既有 bool、Int64 范围整数、有限 float、str、`kat.Duration` 与 `kat.WallClockTimestamp` 转换规则。

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
        self._catalog: ds.Catalog | None = None

    def decode(self):
        if not self._catalog_root.exists():
            with ds.write(
                TRACE_SCHEMA,
                destination=self._catalog_root,
            ) as writer:
                for batch in parse_trace_batches(self._source):
                    writer.write(
                        "events",
                        observed_at=batch.observed_at,
                        thread_id=batch.thread_id,
                        name=batch.name,
                        weight=batch.weight,
                    )

        self._catalog = ds.open(
            TRACE_SCHEMA,
            root=self._catalog_root,
        )
        return self

    def query(self, sql: str, *, params=None) -> ds.Table:
        if self._catalog is None:
            raise RuntimeError("trace source has not been decoded")
        return self._catalog.query(sql, params=params)
```

Parser 算法完全属于 PACK。它可以像上例一样向 Writer 提交 Python 列 batch，也可以运行已有 binary Parser，再用 `ds.open()` 显式绑定其 Parquet 文件或 parts 目录。若 Parser 产生本地数据库，Provider 保留数据库并执行其原生 SQL，不整体转成 Parquet。

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
        return ds.from_arrow(normalize_postgresql_result(arrow_table))
```

Provider 在 Table 返回前已经完整执行远端 SQL 并关闭 query-local cursor、reader 与 transaction。`normalize_postgresql_result()` 表示该具体 Provider 在 bridge 前完成 D41 所需的来源类型规范化：有绝对时间语义的值转成 `timestamp(ns, tz="UTC")`；PostgreSQL `TIMESTAMP WITHOUT TIME ZONE` 没有绝对时间语义，Provider 不得猜成 UTC，必须由来源 SQL 或明确的领域规则解释，否则拒绝形成标准 Table。Provider 可以使用普通 Python context manager 复用连接池，但 KAT 不登记或关闭 Provider。

### 5.4 一个 Workflow 顺序查询两个 Database 并融合本地数据

```python
# workflows/correlate.py
from pathlib import Path

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
    trace = TraceProvider(
        source=Path(trace_path),
        catalog_root=ctx.datasource_root / "trace_catalog",
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

    summary = ctx.sql(
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
        tables={
            "events": events,
            "telemetry": telemetry,
            "owners": owners,
        },
    )

    return {
        "telemetry": telemetry,
        "summary": summary,
    }
```

PostgreSQL 的 `$1` 属于 Provider 来源 SQL；Catalog 的 `$start` 属于 KAT 本地 scalar 参数；`tables` 中的 relation name 只在当前 `ctx.sql()` 调用中有效。`events` 与 `owners` 只作为中间融合输入，不落成 Output；`telemetry` 和 `summary` 因被 Workflow 返回而由 Runtime 发布。单源 Workflow 则可以直接 `return provider.query(...)`，不调用 `ctx.sql()`。

## 6. 已确认决定

### D1：Provider 由 PACK 拥有

Workflow 直接构造或取得 PACK 定义的 Provider，并显式调用其来源能力。目标公共模型删除 `kat.Provider`、`SourceExecutor` 与 `ctx.provider()`；KAT 不创建、继承限制或包装 Provider，也不规定所有 Provider 必须具有同一组方法。

Provider 仍可以组合 KAT 提供的 Schema、Writer、Catalog 和本地查询能力，但这些能力是普通 Toolkit，不是第二个 Provider facade。

### D2：解析算法属于 Provider，KAT 标准化解析结果

用户按文件格式、协议和业务语义自行实现解析。KAT 不理解 Hitrace、Ftrace、文本、远端数据库或自定义二进制格式，也不要求解析器继承统一接口。

KAT 标准化的是解析结果链路：PACK 声明若干逻辑表及其列；解析代码按表提交数据；KAT 校验数据并落成可重新打开和查询的多表 Parquet Catalog。远端数据库可以直接执行自己的 SQL，并在需要落盘或融合时复用同一数据面。

### D3：公共 Datasource Schema 只使用 Python 类型描述表和列

PACK 作者通过 KAT 的薄 Schema API 声明“产生哪些表、每张表有哪些列”，列只使用 Python 类型，不需要直接构造 `pyarrow.Schema`，也不需要选择 `ds.UInt64` 等 KAT 类型标记。KAT Schema 是独立于物理位宽和编码的逻辑类型合同；Arrow 仍是内部批处理、Parquet 与 DataFusion 之间唯一的物理类型事实，不再增加平行的持久 Schema 格式。

首版 Schema 直接使用嵌套 Mapping：外层 key 是表名，内层 key 是保持声明顺序的列名，value 是 Python 类型。`bool`、`int`、`float`、`str`、`bytes`、D44 的 `datetime.datetime` 与 D45 的 `decimal.Decimal` 是首批逻辑类型，`T | None` 表示 nullable。KAT Writer 与 `ds.table()` 对基础 Python 值分别使用 Arrow Boolean、Int64、Float64、Utf8 与 Binary 作为规范物理编码，并按 `T` / `T | None` 产生 non-nullable / nullable 物理字段；datetime 与 Decimal 使用各自固定规范编码。它们不根据首批值推断位宽、有无符号、时间语义、Decimal scale 或空值约束，也不在 Python 类型之间隐式转换。其他逻辑类型只在真实需求时增量加入；这不限制高级来源通过 D26 的 Arrow bridge 形成 D41 准入的 Table。

Schema 只描述解析结果，不描述解析算法。列类型、空值约束与顺序必须在写入前确定，使零行表也能形成具有稳定 Schema 的 Parquet 表。Schema 至少包含一张表、每张表至少一列；`schema.tables` 按声明顺序返回 table name tuple，`schema[name]` 返回可直接交给 `ds.table()` 的只读有序列 Mapping，不增加公共 `TableSchema` 类型。

### D4：`ds.write()` 是显式目标路径的普通库函数

多表落盘使用模块级 `ds.write(schema, destination=...)`，不使用 `ctx.datasource.write(...)`，也不读取隐式当前 Workflow Context。`destination` 必须由调用方显式提供；生产 Workflow 新建的可复用 Datasource 物化必须从 `ctx.datasource_root` 派生 PACK 私有位置，普通 Python 与 PACK 测试则可以直接使用临时目录。

`ds.write()` 负责 Schema 校验、Parquet 写入与零行表保留。它不选择 artifact key、解释来源配置或决定复用策略；这些属于 Provider。它也不发布 Run Output；Workflow 只选择返回哪些 Table，Runtime 独占最终发布。Writer 成功关闭后，调用方通过独立的 `ds.open()` 打开目标，不从已关闭 Writer 取得 Catalog。

### D5：Writer 按列追加 Python 列表批次

首版 Writer 只接受 `writer.write(table_name, /, **columns)`，table name 是 positional-only，避免与任意来源列名冲突。每个 keyword 是 Schema 中的一列，值是本批次的 Python 列表；参数名必须与该表的列集合完全一致，所有列表长度必须相同。一次调用形成一个待校验和写入的 batch，多次调用向同一 Parquet 表继续追加，不覆盖已有批次，也不要求 Parser 把整张表保存在内存中。

KAT 按 Datasource Schema 对每个值执行 D39 的精确 Python 类型校验并立即追加到该表唯一的 `<table>.parquet`，不调用用户对象转换方法，也不做跨批次类型推断。Parser 自己选择批次大小。声明但从未调用 `write()` 的表在 Writer 成功关闭时仍产生一个携带声明 Schema 的零行 Parquet 文件，使空数据与缺失/失败产物可区分。

### D6：Schema 统一逻辑结果，不强制 Parser 使用 Parquet

`ds.write()` 是自定义 Python Parser 的默认落盘方案，不是所有 Parser 的强制出口。已有 binary Parser 若产生多张 Parquet 表，且其物理列都能由 D3、D44 与 D45 的 Datasource Schema 表达，Provider 可以通过 KAT Toolkit 原地打开和校验，不先解码再编码；其他 D41 准入类型由 Provider 查询后通过 Arrow bridge 形成 Table。若 Parser 产生 SQLite 或其他数据库，Provider 保留该数据库及其索引和查询能力，不在 `decode()` 后整体转换为 Parquet。

数据库型 Provider 使用来源自己的 SQL 方言与执行器；某个具体查询选择标准结果路径时，才把该结果形成可供 Python 直接读取、返回为 Run Output 或参加多数据源融合的 `ds.Table`。KAT 不转换整个数据库、不规划来源内 SQL，也不把整个数据库注册到融合 DataFusion。

### D7：`ds.Table` 是可重复读取的标准查询结果

KAT Datasource Toolkit 的标准查询结果是 `ds.Table`。它表示一次已经执行完成、具有确定列结构且可重复读取的单表数据值，不是远端 cursor、惰性 SQL plan、Parquet 路径或已经注册到融合 Session 的 relation。用户可以按列取得 Python sequence，或把整表转换成 Python rows；内部实现可以持有 Arrow Table，普通使用不要求 PACK 作者理解 Arrow，高级来源适配器则可以通过显式 bridge 保留 D41 准入的驱动 Arrow 类型。

`ds.open(...).query()` 返回 `ds.Table`；数据库 Provider 也可以把来源查询结果构造成同一值。Workflow 可以直接计算并返回 `ds.Table` 形成单源 Output，也可以在后续显式步骤中把它交给 KAT 本地数据面参加融合。Provider 仍可暴露来源特有的其他返回值；`ds.Table` 是 Toolkit 的标准路径，不是要求所有 PACK 方法签名相同的 Provider 基类合同。

本决定取代 PR #228 中把 `kat.Table` 定义为已经落盘、具名并绑定当前 operation 的不可构造句柄的模型。融合 relation 的创建、命名与生命周期必须使用另一个显式步骤，不能继续藏在 `Provider.query()` 的副作用中。

### D8：Fusion query 显式接收 Table Mapping

多数据源融合使用 `ctx.sql(sql, tables={name: table})`。Mapping key 是只在该次调用中有效的 Fusion relation name，value 必须是 `ds.Table`；Runtime 在当前 fusion Session 中建立这些临时 relation 后执行 SQL。Provider query 不接收本地结果名、不自动注册，也不留下跨调用 catalog 状态。

该接口不增加 `ctx.register()`、`ctx.table()` 或新的 operation-bound Table 句柄。SQL 引用未出现在显式 Mapping 或迁移期旧 Dataset grant 中的 relation 时直接失败，不发现 Provider、不访问来源 catalog，也不隐式执行来源 SQL。同一个 `ds.Table` 可以直接成为 Run Output，同时作为一次或多次 Fusion query 的输入。

本决定重新打开 `2026-08-26-pack-datasource.md` 中已经确认的“`ctx.sql()` 不传 Table”决定：旧决定依赖 `Provider.query(name=...)` 的自动注册副作用，而 D1 与 D7 已删除这一前提。

### D9：Source query 与 Fusion query 都形成 eager `ds.Table`

`ctx.sql()` 在调用期间立即把显式 Table Mapping 注册到内部 DataFusion Session、执行完整 SQL 并把结果收集为可重复读取的 `ds.Table`。它不再把 DataFusion `DataFrame` 作为新的 Datasource 标准查询结果。来源 `Provider.query()` 与本地 `ctx.sql()` 因而向 Workflow 呈现同一种结果值，结果可以继续作为另一条 Fusion query 的显式输入、由 Python 重复读取或直接成为 Run Output。

eager `ds.Table` 需要结果驻留内存，这是可重复读取且不依赖重读 Parquet的直接代价；首版不再同时提供 lazy/streaming Query Result 变体。Provider 可以在来源内使用数据库或本地 DataFusion 的流式执行来控制中间数据，但进入标准 Query Result 后必须完整形成 `ds.Table`。

迁移期 `ctx.from_arrow()` 与 DataFrame Output 可以继续服务尚未迁移的普通 Arrow authoring path，但 `ctx.sql()` 不再产生 DataFrame，也不根据调用参数切换返回类型。是否最终删除其余 DataFrame authoring path 属于后续迁移计划，不影响本文目标模型。

### D10：Parquet Catalog 同时支持显式表路径与根目录发现

已有 Parquet Parser 可以通过 `ds.open(schema, tables={logical_name: path})` 显式绑定物理布局；每个 path 可以是一张表的 Parquet 文件或包含该表多个 part 的目录。显式 parts 目录递归收集所有 `.parquet` 普通文件、忽略其他扩展名，并要求至少存在一个 part；目录层级只组织文件，不从 `key=value/` 路径推导 Hive partition 列。KAT 原地读取、校验并查询这些路径，不移动、复制或重新编码，也不要求 Parser 生成 KAT Manifest。

对于标准的平铺多表目录，`ds.open(schema, root=directory)` 自动把目录中的每个 Parquet 文件解释为一张逻辑表，表名取文件 stem。例如 `events.parquet` 与 `threads.parquet` 形成 `events`、`threads` 两张表。`root=` 表示多表 Parquet Catalog，不把目录内所有文件合并成一张分片表；单张分片表使用显式 `tables={"events": parts_directory}` 表达。

`root=` 首版只扫描该目录直属的普通 `.parquet` 文件，不递归子目录；其他扩展名文件忽略。嵌套 Parquet 产物必须通过 `tables=` 显式绑定，避免相对路径到表名的额外编码规则和不同子目录中的 stem 冲突。`root` 与 `tables` 必须且只能提供一个；显式 `tables` key 集合也必须与 Schema table 集合完全一致。

`ds.write(schema, destination=...)` 产生一表一文件、可由 `root=` 直接打开的平铺默认布局。两种打开方式都必须按传入的 Datasource Schema 严格校验表集合和每张表的物理 Schema；表集合以及每张表的列名、列顺序和类型必须与声明匹配，nullability 按 D27 的兼容规则校验，缺少、多出或重排都失败。Schema 仍由 Parquet footer 持久化，不增加独立 Manifest。

### D11：`ds.Catalog.query()` 提供标准本地 SQL

`ds.open(...)` 返回可重复使用的 `ds.Catalog`。打开只读取 Parquet metadata、校验 Datasource Schema 并在 Catalog 私有的 DataFusion Session 中注册表路径，不读取全部数据行。Catalog 是对原路径的只读视图，调用方必须在使用期间保持文件存在且内容不变。`catalog.query(sql, params=None)` 使用 KAT 固定版本的 DataFusion 单语句只读 SQL 与具名参数语义，只扫描 SQL 实际引用的表，完整执行后返回不再依赖 Catalog、Session 或源文件的 eager `ds.Table`。

本地文件 Provider 可以直接把 `query()` 委托给 Catalog，从而不重复实现 DataFusion Session、路径注册、参数绑定和结果转换。远端数据库以及数据库型 Parser 产物仍由对应 Provider 使用来源自己的 SQL 方言、参数和执行器；它们不伪装成 Parquet Catalog，也不通过这条本地 Query Engine。

### D12：Provider 生命周期使用普通 Python 规则

KAT 不登记 Provider、不要求 `close()` 协议，也不在 Workflow 结束时自动回收 Provider。eager `Provider.query()` 必须在返回 `ds.Table` 前关闭该次 cursor、reader、临时进程和其他 query-local 资源；来源错误和关闭错误由该 Provider 自己保留正确的主错误语义。

需要复用连接池、客户端或外部进程的 Provider 可以按普通 Python 惯例实现 context manager，由 Workflow 显式使用 `with Provider(...)`。不需要长期资源的 Provider 是普通对象。`ds.write()` 自己的 Writer context manager 继续只管理 Parquet 写入生命周期；首版 `ds.Catalog` 不提供 `close()` 或 context manager，其本地 DataFusion Session 跟随普通 Python 引用释放，不形成必须由 KAT 编排的 Provider 资源协议。

### D13：PACK 新增顶层 `datasources/` 生产模块

PACK 固定源码布局新增可选顶层 `datasources/`，用于放置由该 PACK 拥有的 Provider、Schema 与解析实现。其规范 Python identity 是 `kat.pack.datasources.*`，与 `kat.pack.workflows.*`、`kat.pack.helpers.*` 并列；Workflow 通过普通 Python import 使用其中的 Provider。

这项决定有意修改 ADR-0017 与 ADR-0047 中“生产模块只有 workflows/helpers 两组规范身份”的边界。Datasource 虽然仍是普通 Python 代码，但其来源合同与复用价值已经足以成为 PACK 的一等代码区域；继续放在 `helpers/` 会把稳定来源能力误表达成无领域身份的通用 helper。

Runtime 只把该目录作为 `kat.pack.datasources.*` 规范 namespace 挂载，服从 Python 标准 import；KAT 不扫描、不预加载、不注册 Provider，不要求一文件一个 Provider，也不从 module、类名或 `pack.toml` 推导来源身份。未被 Workflow import 的 Datasource 不在 inspection 时执行，普通 `__init__.py` 与 namespace package 语义均由 Python 决定。该决定由 ADR-0063 记录。

### D14：`ds.Table` 是最小不可变 Python 数据容器

`ds.Table` 首版只提供 `len(table)`、返回稳定列名 tuple 的 `table.columns`、按列返回与内部存储隔离的 Python tuple 的 `table[name]`，以及返回新 `list[dict[str, object | None]]` 的 `table.to_rows()`；同一值可以被反复读取。调用方修改返回的 tuple、list 或 dict 不改变 Table，后续读取仍反映原始不可变数据。普通计算使用 Python 对这些值操作，关系选择、过滤、连接与聚合继续通过把 Table 显式传给 `ctx.sql()` 完成。D42 的高级 Arrow 输入只读合同不改变这些 Python 读取语义。

Table 不提供 `filter()`、`select()`、`group_by()`、表达式系统、原地修改或其他 DataFrame API。内部 Arrow Table 不是普通计算接口，只能经 D26/D43 的显式高级只读 bridge 进出；限制这一职责避免 KAT 重建 pandas、PyArrow compute 或 DataFusion 已有的通用计算层，也保证同一 Table 作为 Python 输入、Run Output 和 Fusion query 输入时保持一致。所有 Table 都要求列名是非空且唯一的字符串；手工构造、Arrow bridge、Source query 或 Fusion query 产生空名或重名列时直接失败，SQL 作者必须使用 `AS` 消除歧义。

### D15：`ds.table()` 从 Python columns 或 rows 构造 Table

手工构造标准查询结果只使用 `ds.table(schema=..., columns=... | rows=...)`。其中 schema 是单表的只读有序 `{column_name: Python type}` Mapping，可以直接使用 `multi_table_schema[table_name]`；`columns` 是与 schema 列集合完全一致的 `{column_name: list}`，`rows` 是按 schema 声明顺序解释的 row sequence，两者必须且只能提供一个。

KAT 使用 D3/D39 的固定类型规则严格校验，拒绝缺列、多列、行宽不一致、列长度不一致、nullability 或值类型错误，完成后形成不可变 `ds.Table`。普通 Python Parser 可以使用 columns，DB-API cursor 的 `fetchall()` 等行式结果可以使用 rows。`ds.Table` 不再提供另一套复杂公共 constructor；`ds.Catalog.query()` 等 KAT 内建路径直接产生同一值，不要求用户重新组装。

### D16：Workflow 返回 Table 决定 Run Output

新的标准 Output 合同是 `ds.Table | dict[str, ds.Table]`。单个 Table 规范化为 `{"main": table}`；dict 必须非空且 key 继续使用 KAT 既有可移植 Output name 规则，并成为 Run Manifest 与 Output Query 中的逻辑名称。Table 自身不携带 Output name，Provider query 也不接收 `name`。不接受任意自定义 Mapping，避免 Runtime 规范化期间观察到动态键值。

Runtime 只在 Workflow 成功返回后把选中的 Table 写成最终 Run Output Parquet，不重新执行来源 SQL。仅作为 Fusion query 输入而未返回的 Table 不产生 Output 文件；同一个 Table 可以先参加融合，再与融合结果一起通过不同 dict key 返回。迁移期继续接受单个 DataFrame 以及非空 `dict[str, ds.Table | DataFrame]`，允许两种值混合；单个旧 DataFrame 同样使用 `main`。空 dict、其他 Mapping 或其他返回类型直接失败，但 `ds.Table` 是 Datasource Toolkit 的新标准值。

### D17：数据库接入统一在 Table 结果边界

KAT 首版不提供通用 DB-API、ADBC 或数据库 executor helper。数据库 SQL 方言、参数、连接与凭据、事务和只读保证、驱动类型转换及资源关闭都由具体 PACK Provider 拥有；Provider 可以把普通 Python rows/columns 交给 `ds.table()`，也可以把驱动已经产生的 Arrow Table 交给 D26 的 `ds.from_arrow()`，形成相同的标准结果。

这避免为了不同数据库的差异重新建立与 SourceExecutor 等价的抽象。仓库可以提交一个使用具体驱动的可运行 `PostgreSQLProvider` 范例，证明远端查询到 `ds.Table` 再到 `ctx.sql()` 的完整链路，但该范例及其驱动不提升为 KAT 核心协议。以后有多个真实 Provider 证明某段数据库胶水稳定复用时，再提炼为可选 Toolkit。

### D18：Writer 正常失败尽力清理，崩溃残留由 Provider 重建

`ds.write()` 要求 destination 开始时不存在，并直接创建目标目录写入。任何 `writer.write()` 失败都会永久 poison 当前 Writer：即使 Parser 捕获该异常，后续 write 与正常 context 退出也继续失败，并保留首个写入错误。Python 异常、poison 或 Writer 正常关闭失败时，KAT 尽力关闭已经打开的表 writer 并删除本次创建的整个目标目录；清理错误只作为附加诊断，不覆盖首个解析或写入错误。

进程崩溃可以留下不完整目录。`ds.open()` 必须重新验证路径、Parquet footer、声明表集合与 Schema，并拒绝在 metadata 层已经可见的不完整产物；数据页损坏在实际查询扫描到该页时失败。KAT 不自动覆盖、合并、恢复或修复已有目录。Provider 遇到残留时显式删除重建或选择新的 artifact key。首版不承诺 crash-safe publication、Manifest complete marker 或原子目录替换，因为这些产物可由来源重新解析。

### D19：打开已有 Parquet 时按 Python 逻辑类型验证

`ds.open()` 不要求已有 Parser 使用 KAT Writer 的所有规范物理编码。Python `int` 接受 Arrow 各宽度有符号或无符号整数，`float` 接受 Float32/Float64，`str` 接受 Utf8/LargeUtf8/Utf8View，`bytes` 接受 Binary/LargeBinary，`bool` 只接受 Boolean；D44 的 `datetime.datetime` 只接受 `timestamp(ns, tz="UTC")`；D45 的 `decimal.Decimal` 接受 Decimal128/Decimal256 的任意受支持 precision/scale。兼容校验不修改或重新编码来源文件，`ds.Table` 对外仍只暴露对应 Python 值。

同一张显式分片表的所有 Parquet part 必须具有一致的物理 Arrow Schema，使 DataFusion 可以把它们作为一张关系读取；一致性比较忽略无业务语义的 Arrow field/schema metadata。KAT Writer 采用 D3、D44 与 D45 的规范编码；这些选择只是新写 Python 数据的确定默认值，不是 binary Parser 的接入格式要求。

### D20：Provider 不接收 Workflow Context

Provider constructor 和方法只接收真正需要的普通来源配置、Schema 与路径，不接收或保存 `kat.Context`。需要 PACK 私有持久化位置时，Workflow 必须从 `ctx.datasource_root` 派生具体路径再传给 Provider；远端数据库 Provider 只接收其连接配置。Provider 不能调用 `ctx.sql()`、访问 Run workspace 或发布 Output。

Context 继续只拥有当前 execution plane、Fusion query 与 PACK 存储根等运行能力，不再承担 Provider factory 或 Datasource Toolkit namespace。这个边界让同一个 Provider 可以在普通 Python 和 PACK test 中直接使用 `tmp_path` 验证，也避免 Provider 生命周期重新绑定 Execution Lease。

### D21：典型文件 Provider 显式准备内部 backend

推荐的文件 Provider 是一个实例对应一个具体来源的有状态普通类。`decode()` 显式执行自定义 Python Parser 或已有 binary Parser；若产物是 Parquet，它在 Writer 成功关闭或 binary Parser 成功返回后显式调用 `ds.open()`，并把形成的 `ds.Catalog`、数据库路径或其他 backend 保存为 Provider 私有状态。`query()` 随后使用该 backend，尚未成功 decode 时直接报告来源未准备。

`decode()` 返回 `self` 以允许可选链式调用，但 Workflow 可以分两步显式调用。`ds.Catalog` 是 Provider 作者使用的标准本地实现，不要求暴露给普通 Workflow；Provider 可以把 `query()` 委托给它。无需解析的远端数据库 Provider 构造后即可 query。这是示例和作者约定，不是 KAT 用反射检查的 Provider 协议。

### D22：每次 Fusion query 使用隔离的 DataFusion Session

每次 `ctx.sql()` 创建独立、短命的 DataFusion Session，只注册本次显式 `tables` Mapping、必要的 Session 配置与迁移期旧 Dataset grants，完整执行并形成 `ds.Table` 后释放。Fusion relation name 只属于该次调用；不同调用可以复用同名并绑定不同 Table，不存在跨调用 register/deregister 或 mutable catalog。

前一次结果若要继续参与计算，Workflow 必须在下一次 `tables` Mapping 中显式传入。SQL 失败只结束当前调用，不 poison Context；Workflow 可以捕获后使用另一条 SQL 或另一组 Table 重试。PACK 仍不能取得底层 SessionContext。本决定取代 PR #228 为整个 operation 维护 Provider Table catalog、名称保留和失败 poison 状态的设计。

### D23：Fusion query 分离 Table 与 scalar 参数

`ctx.sql(sql, *, tables=None, params=None) -> ds.Table` 使用两个显式 Mapping：`tables` 把 call-local relation name 绑定到 `ds.Table`，`params` 提供 DataFusion `$name` scalar 参数；两者缺省均为空。接口不再把 scalar 作为 `**params` 接收，避免动态参数名与 `tables`、`params` 或未来 keyword 冲突。

Scalar 类型继续复用现有 `ctx.sql()` 已准入的 Python 值、`Duration` 与 `WallClockTimestamp` 转换，不为 Datasource 另建参数类型系统。SQL 文本、Table Mapping 和参数 Mapping 都只供本次调用使用，不记录到 Provider、Table 或全局 catalog。

### D24：Fusion relation 与 parameter 使用可移植名称

`tables` key 与 `params` key 都必须满足小写 SQL-friendly 名称规则 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`。它们不形成文件名，因此不额外排除 Windows device name；两者属于不同 namespace，可以使用相同文本，Mapping 自身保证各自名称唯一。KAT 不复制 DataFusion SQL 关键字列表，罕见关键字冲突由 PACK 在 SQL 中正确引用或改名。

Runtime 在创建 call-local Session 时先注册迁移期旧 Dataset grants，再检查显式 Table Mapping；同名时在执行 SQL 前失败，不覆盖或 shadow Dataset relation。SQL 引用不存在的 relation 或 parameter 也只使本次查询失败，不触发 Provider、来源发现或 Context poison。

### D25：`ctx.sql()` 做一次明确的破坏性迁移

新的 `ctx.sql(sql, *, tables=None, params=None) -> ds.Table` 直接替换既有 `ctx.sql(sql, **params) -> DataFrame`，不保留按参数形态或调用上下文切换返回类型的双模 overload。同一方法始终使用显式 Table Mapping 与参数 Mapping，并始终 eager 返回 `ds.Table`，使调用方不需要猜测返回的是 Table 还是 DataFrame。

实现该切片时必须同步迁移仓库内所有旧 `ctx.sql(..., **params)`、依赖惰性 DataFrame 的 `.collect()` 及其测试；不能把旧调用留给运行时兼容分支。迁移期保留的 `ctx.from_arrow()` 与 DataFrame Output 是另一条显式旧路径，不改变 `ctx.sql()` 的新合同。本决定由 ADR-0064 记录。

### D26：高级来源可以从 Arrow 构造标准 Table

Datasource Toolkit 提供 `ds.from_arrow(table: pyarrow.Table) -> ds.Table`。数据库驱动、binary Parser 或其他高级适配器已经取得 Arrow Table 时，可以通过该入口保留 D41 准入的列名、顺序、物理类型与 nullability，包括 `timestamp`、`decimal` 等丰富类型；Arrow field/schema metadata 不属于标准 Table 合同。调用方不需要先转成 Python rows，也不需要为动态查询结果手工重建 Datasource Schema。若来源交付 `RecordBatchReader`，Provider 先完整读取为 Arrow Table，再调用该入口，因此返回值仍满足 D9 的 eager、可重复读取合同；buffer 所有权遵守 D42。

这一高级 bridge 不扩大普通 `ds.Schema`、`ds.write()` 与 `ds.table()` 首版支持的七种 Python 逻辑类型，也不要求普通 Parser 作者理解 Arrow。由 bridge、Catalog query 或 Fusion query 形成的 `ds.Table` 可以承载 D41 中经验证的更多结果类型；Python 列读取、融合与 Output publisher 必须保持这些类型。既有 `ctx.from_arrow()` 暂时保留原有 DataFrame 语义，与返回标准 Table 的 `ds.from_arrow()` 是两个显式入口，不按输入动态切换。

### D27：已有 Parquet 的 nullability 只按 metadata 校验

`ds.open()` 校验已有 Parquet 时，逻辑必填列 `T` 只接受物理字段 `nullable=false`；逻辑可空列 `T | None` 同时接受 nullable 与 non-nullable 物理字段，后者只是提供了更强保证。KAT 不扫描数据页来证明一个 nullable 字段当前恰好没有 null，也不据此把它提升为逻辑必填列。

这一规则与 D11 的 metadata-only open 保持一致，使兼容性只依赖稳定 Schema 而不依赖某一批文件内容。实际查询若遇到数据页损坏仍在扫描时失败，但 nullability 合同不会推迟到查询阶段才验证。

### D28：Datasource Toolkit 只通过 `kat.datasource` 暴露

KAT 的标准数据面模块是 `kat.datasource`，PACK 推荐使用 `from kat import datasource as ds`。首版由该模块公开 `Schema`、`Table`、`Catalog`、`table()`、`from_arrow()`、`to_arrow()`、`write()` 与 `open()`；本文中的 `ds.*` 均是这个规范模块的别名，不是 PACK 下 `datasources/` 源码目录的对象。

新类型与函数不再平铺为 `kat.Table`、`kat.Schema` 等顶层名称。PR #228 引入的顶层 `kat.Provider`、`kat.SourceExecutor`、`kat.ParquetSource` 与 operation-bound `kat.Table` 按 D1 删除，不保留双入口或兼容 alias。PACK 的 `kat.pack.datasources.*` 拥有来源实现，KAT 的 `kat.datasource` 只提供可组合 Toolkit，两者不会互相扫描、注册或包装。

### D29：Writer 成功后由调用方显式 `open`

`ds.write()` 的 context manager 只拥有一次多表写入。成功退出表示目标目录中的 Parquet 已完整关闭，但不返回、不缓存也不暴露 `writer.catalog`；已关闭 Writer 不转换成另一种可查询对象。需要查询时，Provider 随后显式调用 `ds.open(schema, root=destination)`，并自行决定是否保存返回的 `ds.Catalog`。

因此刚写出的产物与已有 Parser 产物通过同一个 `open()` 边界完成表集合、footer 与 Schema 校验。Writer、Catalog 的失败语义和生命周期保持独立，也不需要设计 context manager 退出值或 after-close 隐藏状态。

### D30：Datasource Schema 是不可变的严格合同

`ds.Schema(...)` 在构造时复制并冻结输入声明，调用方随后修改原始 Mapping 不影响 Schema。`ds.open()` 要求实际表集合与声明完全一致，每张表的列名、列顺序与逻辑类型必须匹配，nullability 按 D27 的兼容规则校验；缺少或额外的表/列、列重排、类型不兼容都直接失败。

KAT 不在打开时自动重排、补列、删列或 cast，Provider 若要适配已有 Parser 布局必须在 Schema、显式 table binding 或自身解析流程中明确完成。单张分片表的所有 part 还必须具有一致物理 Arrow Schema；比较时忽略 Arrow field/schema metadata，但不忽略字段顺序、物理类型与 nullability。

### D31：只对路径与 SQL identity 施加必要名称约束

Datasource Schema 的表名会成为 KAT Writer 默认布局中的 `<table>.parquet`，因此复用既有 Output/table name 规则：满足 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，并排除 `con`、`prn`、`aux`、`nul`、`com1`—`com9` 与 `lpt1`—`lpt9` 等 Windows device name。Schema table Mapping 与 `ds.open(..., tables=...)` 的 key 都使用这一规则。

列名不形成路径，只要求是非空字符串并按大小写精确匹配，不强制 snake_case，也不清洗或规范化已有 Parquet 列名；非简单标识符在 SQL 中由 PACK 正确引用。Fusion relation 与 parameter 按 D24 使用小写 SQL-friendly 正则，但由于不落成文件，不额外排除 Windows device name。Output name 继续使用完整 file-safe 规则。

### D32：Table 只暴露隔离的 Python 列与字典行

`ds.Table` 的首版 Python 表面固定为 `len(table) -> int`、`table.columns -> tuple[str, ...]`、`table[name] -> tuple[object | None, ...]` 与 `table.to_rows() -> list[dict[str, object | None]]`。`to_rows()` 每次创建新的 list/dict，列读取也不暴露内部可变存储；调用方修改返回容器不改变 Table，后续读取保持原始结果。UTC nanosecond timestamp 投影为保留九位小数精度的 `kat.WallClockTimestamp`，Decimal128/Decimal256 投影为 `decimal.Decimal`，不经 Python `datetime` 或 float 中转。

所有 Table 都要求列名是非空且唯一的字符串。`ds.table()`、`ds.from_arrow()`、Catalog query 与 Fusion query 形成重名或空名列时，在返回标准 Table 前失败；SQL 查询需要使用 `AS` 明确消除重名。Table 不提供 mutation、filter/select/group-by、表达式或其他 DataFrame API。

### D33：Catalog 依赖稳定源路径，Table 与源路径脱离

`ds.Catalog` 是 `ds.open()` 所绑定原 Parquet 路径的只读 live view，不复制文件或形成快照。调用方必须在 Catalog 使用期间保持文件存在且内容不变；外部删除或修改路径属于调用方违约，KAT 不检测也不保证后续结果，查询可能失败，也可能读取到同路径的新内容，且不会缓存旧文件内容来维持快照。

`catalog.query()` 返回的 eager `ds.Table` 已经与 Catalog、私有 DataFusion Session 和源文件完全脱离；Catalog 引用释放或源文件随后删除，都不影响既有 Table 的 Python 读取、Fusion query 或 Output。首版 Catalog 不提供 `close()` 或 context manager，内部 Session 跟随普通 Python 引用释放。

### D34：显式 parts 目录递归聚合一张表

`ds.open(schema, tables={name: path})` 中的 path 可以是单个 Parquet 文件，也可以是一张逻辑表的 parts 目录。parts 目录递归收集后缀为 `.parquet` 的普通文件并按稳定相对路径顺序校验，忽略其他扩展名文件，且至少必须包含一个 part；所有 part 继续遵守 D19/D30 的一致物理 Schema 合同。

目录名只用于组织文件，KAT 不从 `date=.../`、`cpu=.../` 等层级推导 Hive partition 列。该模式与 `root=` 的平铺多表发现严格区分：`root=` 只读取直属 Parquet 文件并把每个文件 stem 解释成独立表。`root` 与 `tables` 必须且只能提供一个，显式 `tables` key 集合也必须精确等于 Schema table 集合。

### D35：两个本地 SQL 入口共用单语句只读准入

`ds.Catalog.query()` 与 `ctx.sql()` 共用 KAT 固定版本的 DataFusion SQL admission：只接受一条只读语句，允许 `SELECT`、`WITH`、`VALUES`、`DESCRIBE`、`EXPLAIN`，以及结果物理类型满足 D41 的只读 `SHOW` 变体；拒绝多语句、DDL、DML、`COPY` 与 Session 状态修改。DataFusion 54 的 `SHOW FUNCTIONS` 会产生 `list<string>` 列，因此即使语句只读，也在标准 Table admission 处拒绝。两者也共用 D23/D24 的 scalar 转换与参数名称规则。

本地 SQL 的解析、planning 或执行失败只结束当前调用，不 poison Catalog 或 Context，后续可以使用其他 SQL 重试。这一准入只约束 KAT 拥有的本地 DataFusion；远端或数据库 `Provider.query()` 的方言、允许语句、事务和读写策略仍完全属于 Provider，KAT 不预先解析其 SQL。

### D36：Workflow 用单值或非空普通 dict 选择 Output

新的标准 Workflow 返回值是 `ds.Table | dict[str, ds.Table]`。单个 Table 规范化为 `{"main": table}`；dict 必须是精确内建 `dict`、必须非空，并用满足完整 file-safe Output name 规则的 key 命名每个 Table。Table 自身没有 Output name，Runtime 只在 Workflow 成功返回后物化这些最终选择，不为中间 Table 提前创建 Output。

迁移期继续接受单个旧 DataFrame，以及非空的精确 `dict[str, ds.Table | DataFrame]`，允许新旧值混合；单个 DataFrame 也规范化为 `main`。任意自定义 Mapping、空 dict 或其他值失败，避免 Output 规范化期间观察动态键值，也不增加“零 Output Workflow”语义。

### D37：Schema 只提供表列表与单表列 Mapping

`ds.Schema` 至少包含一张表，每张表至少包含一列。构造时复制并冻结嵌套声明后，`schema.tables` 按声明顺序返回 `tuple[str, ...]`，`schema[table_name]` 返回该表的只读、有序列 Mapping；未知表名使用普通 Mapping 语义抛出 `KeyError`。KAT 不公开额外的 `TableSchema` 类型或 mutation API。

`schema[name]` 可以直接作为 `ds.table(schema=...)` 的单表合同。Writer 的入口固定为 `writer.write(table_name, /, **columns)`，使 table name 参数不会与名为 `table_name` 的来源列冲突；Schema 仍允许 D31 已确认的任意非空列名，调用方可用 `**column_mapping` 提交不能写成 Python keyword literal 的列。

### D38：生产物化归入 `ctx.datasource_root`，Toolkit 不读取全局根

生产 Workflow 新建并希望复用的 Datasource 物化必须位于当前 PACK 的 `ctx.datasource_root` 下。Workflow 从该根派生具体 artifact 路径并把普通 `Path` 传给 Provider；Provider 不接收 Context。最终 Run Output 继续由 Runtime 写入候选 Run 的受管理目录，不由 Provider 或 `ds.write()` 选择位置。

`kat.datasource` 是可在普通 Python 与 PACK 测试中使用的纯路径 Toolkit，因此 `ds.write()` 不读取环境变量、隐式当前 Context 或全局 KAT Data Home，也不机械证明传入路径属于 `ctx.datasource_root`；测试可以使用 `tmp_path`。外部输入与已有 Parser 产物也可以从任意获准的只读路径 `ds.open()`。这一边界是 Pack Authoring 合同，不是 Python 文件系统沙箱。

### D39：Python Schema 路径只接受精确值类型

`ds.write()` 与 `ds.table()` 对每个非 null 值使用精确 Python 类型规则：`bool` 只接受 `type(value) is bool`；`int` 只接受非 bool 的精确 `int` 并要求在 Int64 范围；`float` 只接受精确 `float`；`str` 与 `bytes` 只接受各自精确类型；`datetime.datetime` 按 D44 校验时区与范围；`decimal.Decimal` 按 D45 校验精度、scale 与精确性。`None` 只允许出现在 `T | None` 列。

KAT 不把 bool 当 int、不把 int 自动转成 float、不接受 bytearray 代替 bytes，也不调用用户对象的 `__int__`、`__float__`、`__str__` 等转换方法。类型错误在当前 write batch 或 `ds.table()` 构造期间失败，不等到 Parquet close 或查询。驱动与高级 Parser 已经形成的类型继续通过 `ds.from_arrow()` 保留。

### D40：标准 Writer 固定为平铺的一表一文件

`ds.write()` 首版固定产生 `destination/<table>.parquet`：每张 Datasource Schema 表恰好对应一个 Parquet 文件，多次 `writer.write()` 向同一文件追加 batch，零行表也保留一个带 Schema 的文件。Writer 不为 batch 生成 part，也不公开分片数、文件命名、压缩算法或 row-group 配置；这些使用 KAT 选择的稳定实现默认值。

需要分片、嵌套目录、特定压缩或其他物理布局的 binary/custom Parser 可以绕过 Writer 自行产生 Parquet，再通过 `ds.open(..., tables=...)` 接入。因而固定的是标准 Python Writer 的低门槛路径，不是所有 Datasource Provider 的存储协议。

### D41：标准 Table 与 Output Query 使用分层类型准入

标准 `ds.Table` 首版只允许经锁定版本 PyArrow、DataFusion 与 Parquet 共同验证的扁平列类型：Boolean、各宽度有符号/无符号整数、Float16/Float32/Float64、Utf8/LargeUtf8/Utf8View、Binary/LargeBinary、`timestamp(ns, tz="UTC")` 与 Decimal128/Decimal256，以及它们的 nullable 字段。Table 至少包含一列并继续满足非空唯一列名；每条创建路径还必须检查每个 non-nullable 字段对应的整个 Arrow ChunkedArray 的总 `null_count == 0`。

union、extension、list、struct、map、date、time、duration 与其他未列出的类型首版拒绝，Provider 或 SQL 必须在形成标准 Table 前显式转换。`ds.table()`、`ds.from_arrow()`、`ds.Catalog.query()` 与 `ctx.sql()` 都在返回前执行同一 Table admission；不能创建一个 Python 可读但无法交给 DataFusion 或 Parquet Output 的半有效 Table，也不把不支持类型的错误延迟到 Workflow 返回后的发布阶段。

已发布 Output 的后续 Output Query 还受现有 JSON Query Result 边界约束。它只准入 Null、Boolean、各宽度整数、有限 Float16/Float32/Float64、Decimal128/Decimal256、Utf8/LargeUtf8/Utf8View 与 `timestamp(ns, tz="UTC")`；Int64/UInt64、Decimal 与 timestamp 分别按既有无损字符串规则输出。Binary 等较宽 Table 类型可以正常融合和发布，但用户必须在 Output Query SQL 中显式投影或 cast 为受支持的 JSON scalar，不能把“可发布 Table”误解为“任意列都可直接返回 JSON”。

### D42：Arrow bridge 使用只读 buffer 移交，不深拷贝整表

`ds.from_arrow()` 不为不可变快照无条件深拷贝整个 Arrow Table。KAT 保持输入 Table 及其 buffers 的强引用；调用后 Provider 可以继续读取原 Table，但不得修改其背后的外部可变 buffer。违反这一只读移交合同后的行为不受保证。Catalog query 与 Fusion query 由 KAT 自己形成和持有 buffers，自动满足该合同。

bridge 在构造时执行 Arrow 完整结构校验并复用 D41 的共同 Table admission，不把非法 null 延迟到 Parquet publisher。它保证列名、顺序、物理类型与 nullability，不把 Arrow field/schema metadata 纳入标准 Table 合同。标准 `table[name]` 与 `to_rows()` 继续返回和 backing 隔离的 Python 值，不把 Arrow buffer 暴露给普通读取路径。

### D43：`ds.to_arrow()` 提供对称的高级只读桥

Datasource Toolkit 提供 `ds.to_arrow(table: ds.Table) -> pyarrow.Table`，与 D26 的 `ds.from_arrow()` 对称。它共享 Table 已有的只读 buffers，不复制整表；调用方不得修改返回 Arrow Table 背后的 buffer。普通作者仍使用 `table[name]` 与 `to_rows()`，这一 module-level bridge 不给 Table 增加 DataFrame 或 compute 方法。

该桥使 eager `ctx.sql()` 结果仍可显式进入既有 `ctx.from_arrow()` DataFrame 路径，例如继续使用只接收 DataFusion `Expr` 的 `ctx.convert_clock()`；KAT 不因此注册时钟 SQL UDF，也不改变 `ctx.from_arrow()`、`ctx.convert_clock()` 或 DataFrame Output 的迁移语义。需要重新形成标准 Table 时，高级调用方可以完整 collect Arrow 后再使用 `ds.from_arrow()`。

### D44：Python `datetime` 固定表示 UTC nanosecond timestamp

Datasource Schema 首版接受 `datetime.datetime` 与 `datetime.datetime | None`。非 null 值必须是精确、带有效 UTC offset 的 Python datetime；KAT 在写入前把它换算到 UTC，规范物理类型固定为 Arrow `timestamp(ns, tz="UTC")`。naive datetime 直接失败，不读取本机时区，也不把缺失 offset 猜成 UTC。

`ds.open()` 中的 `datetime.datetime` 同样只接受物理 `timestamp(ns, tz="UTC")`，其他单位、timezone 或无时区 timestamp 必须由 Provider/来源 SQL 显式 cast 后再形成标准数据面。值还必须落在 Arrow signed Int64 nanosecond timestamp 范围内。Table 的 Python 读取使用既有 nanosecond formatter 直接形成 `kat.WallClockTimestamp`，不调用 PyArrow 的 timezone-to-`datetime` 转换，从而保留纳秒并避免依赖主机 timezone database。该语义与 DataFusion scalar 和 Output Query 的绝对 UTC 时间边界一致。

### D45：Python `Decimal` 使用固定 Writer 编码并兼容已有 Decimal

Datasource Schema 首版接受 `decimal.Decimal` 与 `decimal.Decimal | None`。`ds.write()` 与 `ds.table()` 的规范物理类型固定为 Arrow `decimal128(38, 18)`：只接受有限的精确 Decimal；可以补零，或仅删除不改变数值的尾随零，以无舍入方式精确 rescale 到 18 位小数。rescale 后最多保留 20 位整数与 18 位小数；任何需要四舍五入、NaN、Infinity 或超出 precision/range 的值立即失败，不从首批数据推断 scale。Table 的 Python 读取保持 `decimal.Decimal`。

`ds.open()` 的逻辑 Decimal 接受 D41 准入的 Decimal128/Decimal256 及其既有 precision/scale，不重写物理类型。首版不增加 `ds.Decimal(precision, scale)`、`typing.Annotated` metadata 或第二种 Schema DSL；若固定 Writer 编码不能覆盖后续真实 Python Parser，再从同一个 Python Schema 模型增量扩展。

## 7. 迁移范围

### 7.1 公共接口迁移

本设计对尚未合并的 PR #228 进行直接替换，不为该 PR 新增的 facade 接口保留兼容层：

- 删除顶层 `kat.Provider`、`kat.SourceExecutor`、`kat.ParquetSource` 与 operation-bound `kat.Table`；
- 删除 `ctx.provider()`、Provider query 自动命名、自动注册、提前写入候选 Output 和 operation catalog；
- 新增唯一 Toolkit namespace `kat.datasource`，只从这里公开 D28 列出的能力；
- `ctx.sql(sql, *, tables=None, params=None) -> ds.Table` 直接替换 `ctx.sql(sql, **params) -> DataFrame`，不提供双模 overload；
- 仓库内全部 `ctx.sql(..., **params)`、依赖其惰性结果的 `.collect()` 与相应测试必须在 PR #228 内同步迁移；
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

新的 `ctx.sql()` 每次创建独立 Fusion Session，注册本次显式 `tables` 与迁移期 Dataset grants，执行完成后形成与 Session 脱离的 eager `ds.Table`。Workflow 调用前仍执行既有 Required-table grant 检查，不能因为改成 call-local Session 而把 Dataset 授权或输入错误推迟到任意 Provider 行为之后。

### 7.3 仓库内代码迁移

| 当前区域 | 目标状态 |
|---|---|
| `kat/platform/workflow/api/_datasource.py` | 用 `kat.datasource` 的 Schema、Table、Catalog 与构造函数替换 Provider facade 模型 |
| `kat/platform/workflow/api/_workflow.py` | 删除 `provider()`，替换 `sql()` 签名和返回类型，保留 `from_arrow()`、`convert_clock()` 与 `datasource_root` |
| `kat/platform/workflow/runtime/datasource.py` | 删除 Provider operation 状态机；按职责把可复用实现收敛到 Toolkit、Fusion 与 Output 边界 |
| `kat/platform/workflow/runtime/execution.py` | 分离 call-local Fusion Session 与迁移期 DataFrame execution path |
| `kat/platform/workflow/runtime/outputs.py` | eager Table 只在 Workflow 返回后写入最终 Output；继续支持迁移期 DataFrame 与混合 dict |
| `examples/packs/local-parquet-fusion` | Provider 移到顶层 `datasources/`，通过 `ds.open()`、`Catalog.query()` 与显式 `ctx.sql(tables=...)` 完成本地融合 |
| `kat-openharmony-thread-cpu-time` | `ctx.sql()` 结果直接作为 `ds.Table` Output |
| `kat-openharmony-critical-path` | SQL 参数改为 `params={...}`，按需要使用 `Table.to_rows()` 或显式 Arrow/DataFrame bridge |
| PR #228 Provider 测试 | 删除 facade、自动注册、name/backing、poison 与 executor close 合同，改测 Toolkit、call-local Fusion 与最终 Output |
| PostgreSQL 案例分支 | 在后续 PR 基于新 PR #228 重写，不把旧 `SourceExecutor` 兼容层带入新设计 |

Issue #226、PR #228 描述与相关 ADR 必须先同步为本文模型，再实现代码；交付说明不能继续承诺 Provider facade、自动注册或 lazy `ctx.sql()`。

## 8. 两阶段交付

| 切片 | 交付内容 | 明确不包含 | 完成证明 |
|---|---|---|---|
| PR #228：Toolkit 与本地融合 | `kat.datasource`；Schema、Table、`table()`、Arrow bridge、Writer、`open()`、Catalog；顶层 PACK `datasources/`；call-local eager `ctx.sql()`；Table Output；仓库内 breaking migration；Bundled PACK 迁移；本地 Parquet Provider 与融合案例 | PostgreSQL Provider、ADBC 依赖、远端服务测试；旧 Dataset/Import 删除；Hitrace/Ftrace 迁移 | Python 合同测试、Runtime 子进程测试、External PACK 用户链、Bundled PACK 回归、Linux/Windows Full CI |
| 后续 PostgreSQL PR | 顶层 `datasources/postgresql.py` 普通 Provider；锁定 ADBC 依赖；动态 Arrow 结果经 `ds.from_arrow()` 形成 Table；同一 service 依次查询两个 Database 并与本地 Parquet 融合；作者 README；真实服务测试 | 通用 DB executor、数据库 registry、统一事务协议、透明 federation、Docker 生命周期管理 | Linux 与 Windows 的真实 PostgreSQL Provider 合同、External PACK inspect/test/run/query 全链与脱敏证据 |

PR #228 必须独立形成可运行的本地纵向切片。后续 PostgreSQL PR 不重新设计核心接口，只证明普通远端 Provider 能复用同一 Table、Output 与 Fusion 边界。

## 9. 明确不做

- 不建立 Provider 基类、Protocol、facade、factory、registry、decorator、entry point、Binding 或平台发现机制；
- 不自动扫描或预加载 PACK `datasources/`；
- 不实现透明跨源 SQL、SQL 拆分、自动下推、Federation planner、远端 TableProvider 或成本优化；
- 不统一远端数据库的 SQL 方言、参数、事务、驱动、连接池与资源协议；
- 不提供通用 DB-API、ADBC executor 或数据库 helper；
- 不建立 Parser 基类或 Parser registry，也不规定 Provider 必须暴露统一方法集合；
- 不要求 binary/custom Parser 使用 `ds.write()`，也不把数据库型解析产物整体转换为 Parquet；
- 不提供 lazy 或 streaming 标准 Table；`ds.Table` 与两个本地 SQL 入口均为 eager；
- 不把 `ds.Table` 扩张成 DataFrame，不增加 filter、select、group-by、表达式或 mutation API；
- 不在首版扩张 D3、D44、D45 的七种 Python Schema 逻辑类型；高级类型只通过 Arrow bridge 或查询结果保留；
- 不在本切片删除 `ctx.from_arrow()`、DataFrame Output、旧 Dataset、`required_tables`、Test Dataset 或 `kat import`；
- 不迁移现有 Hitrace/Ftrace/native Parser，也不改变 Rust Datasource 的现有 Import 合同；
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
| `ds.table()` | rows/columns 二选一；列集合、顺序、长度、row width、精确值类型、Int64 范围与 nullability 严格失败；结果可重复读取且不受返回容器修改影响 |
| Arrow bridge | 保留名称、顺序、物理类型与 nullability；共享 buffer 的只读所有权；所有 non-nullable ChunkedArray 的总 null count；拒绝空名、重名、嵌套与未准入类型；metadata 不构成合同 |
| Table 类型 | D41 的精确物理类型矩阵；UTC ns 读取为 `WallClockTimestamp` 且保留九位精度；Decimal 读取为 `Decimal`；Python 读取不依赖主机 timezone database |
| datetime 合同 | aware datetime 按绝对 instant 规范到 UTC；naive、无有效 offset 与 signed Int64 ns 越界立即失败；Writer 固定 `timestamp(ns, tz="UTC")`；Arrow/Catalog 只接受同一物理类型 |
| Decimal 合同 | Writer 固定 `decimal128(38,18)`；补零或只删尾随零的 exact rescale 成功；需舍入、NaN、Infinity 与 precision/range 越界失败；open/from_arrow 保留准入的 Decimal128/256 precision、scale 与值 |
| Writer | 多表、多 batch、任意写入顺序、零行表、一表一文件；destination 已存在失败；异常与 close 失败尽力清理整个目标目录 |
| `ds.open(root=...)` | 只发现直属 Parquet 文件；忽略其他扩展名；表集合必须与 Schema 完全一致 |
| `ds.open(tables=...)` | 文件与递归 parts 目录；稳定 part 顺序；至少一个 part；忽略非 Parquet 文件；不推导 Hive partition 列 |
| Schema 兼容 | 列集合与顺序严格；D19 物理类型矩阵；required/nullable metadata 规则；所有 parts 物理 Schema 一致；忽略 Arrow metadata；损坏 footer 在 open 时失败 |
| Catalog | open 不扫描全部数据行；多次 query 可复用；只读单语句与具名参数；失败后可重试；源路径被外部删除或替换后的新 query 可以失败或看到同路径新内容，但不得伪装为旧快照 |
| Table 脱离 | Catalog query 返回后，释放 Catalog 或删除源文件不影响既有 Table 的 Python 读取、Fusion 或 Output |
| `ctx.sql()` | `tables` 与 `params` 分离；关系名 call-local；每次 Session 隔离；Dataset grant 冲突在执行前失败；缺表、缺参数、非法名称与非法 SQL 只影响当前调用；后续调用可重试 |
| `ctx.datasource_root` | 生产值位于当前 KAT Data Home 的 PACK 私有范围且只读、受 Lease 约束；Workflow 只把派生 Path 传给 Provider；同一 `kat test` 中多次 `kat_run` 可复用同一测试根，不同 test 根彼此隔离且不写生产 Data Home |
| SQL 准入 | 两个本地入口都接受 SELECT/WITH/VALUES/DESCRIBE/EXPLAIN 与结果满足 D41 的只读 SHOW；`SHOW FUNCTIONS` 因嵌套列失败；多语句、DDL、DML、COPY 与 Session mutation 被拒绝 |
| Output | 单 Table 使用 `main`；非空普通 dict 显式命名；支持 Table/DataFrame 混合；中间 Table 不产生 Output；来源 SQL 不重复执行；零行 Table 保留 Schema |
| Output Query | D41 的 wider Table 可以发布；JSON Query Result 只接受其更窄标量集合；Binary 等列必须先在 SQL 中显式投影；非有限 float 失败 |
| DataFrame 迁移 | `ctx.from_arrow()`、`convert_clock()` 与 DataFrame Output 现有测试继续通过；`ctx.sql()` 永远不返回 DataFrame |
| PACK namespace | `kat.pack.datasources.*` 的普通 import、namespace package 与可选 `__init__.py` 行为；不扫描、不预加载；副作用 Workflow 注册仍被入口规则拒绝 |
| Bundled PACK | thread CPU time 的 Table Output 与 Output Query 保持可用；critical-path 迁移后全部 PACK 测试通过 |
| 本地纵向案例 | External PACK 完成 inspect、test、run 与 Output Query；两个本地来源先分别查询，再显式传给 `ctx.sql()` 融合 |
| 平台回归 | Linux 与 Windows Full CI 都通过，不以单平台结果代替 |

建议至少记录以下实际证据：

```text
python -I -B -m unittest discover -s kat/platform/workflow/tests -p "test_*.py"
cargo test --workspace --features kat-datasource/protobuf-source-contract-fixture --locked
cargo test --locked -p kat-cli --test trace_streamer_demo trace_streamer_demo_runs_the_full_user_loop -- --ignored --exact
```

真实 Host 条件下还应执行两个 Bundled PACK 的 `kat test`，以及本地案例 README 中的 inspect、test、run、query 命令。PR 描述必须记录实际命令、平台与结果，不能只列计划。

### 10.2 后续 PostgreSQL PR

| 边界 | 必须验证 |
|---|---|
| Provider 形态 | `PostgreSQLProvider` 是 PACK 普通类，不继承 KAT 类型、不接收 Context、不调用 `ctx.sql()` |
| Source query | SQL 与位置参数交给 PostgreSQL；同库 Join、Filter、Aggregate 在远端完成 |
| Table bridge | ADBC 结果完整读取后经 `ds.from_arrow()` 返回 eager Table；零行仍保留结构；Provider 在 bridge 前把有绝对时间语义的 timestamp 规范为 D41 的 `timestamp(ns, tz="UTC")`，或让来源 SQL 直接返回该类型；decimal、整数、浮点、布尔、文本与 null 按 D41 保真 |
| 单源 Output | Workflow 可直接返回 PostgreSQL Table，不执行第二次远端 SQL |
| 多库融合 | 同一 service 下依次查询两个不同 Database，并把各自 Table 与本地 Parquet Table 显式传给 `ctx.sql()` |
| 资源生命周期 | Table 返回前 cursor、reader、transaction 与 query-local connection 已关闭；若 Provider 选择复用长期资源，则由普通显式 context manager 管理 |
| 只读与参数 | 服务端只读事务成立；写权限测试角色仍无法执行 DML、DDL 与 COPY；参数绑定不做 SQL 文本替换 |
| 错误语义 | 连接、认证、SQL、读取与关闭失败不泄露 service、Database、密码或连接字符串；Workflow 可按普通 Python 规则处理 Provider 错误 |
| 用户链 | 生产形态 example PACK 完成 inspect、test、run 与 `kat query --run`，不使用 test-only Provider |
| 平台 | Linux、Windows 分别连接真实 PostgreSQL 服务执行同一合同与完整案例；测试未执行或被 skip 不构成支持证据 |

建议记录真实环境命令：

```text
kat test --pack-dir ./examples/packs/postgresql-parquet-fusion
cargo test --locked -p kat-cli --test postgresql_parquet_fusion_demo postgresql_parquet_fusion_demo_runs_the_full_user_loop -- --ignored --exact
```

以上 PostgreSQL 命令必须在 Linux 与 Windows 各执行一次，并在 PR 中记录实际通过结果、Workflow Host wheel、服务配置方式，以及秘密未进入 stdout、stderr、Operation log、Runtime Response、Run Manifest 与 KAT Data Home 的检查结果。
