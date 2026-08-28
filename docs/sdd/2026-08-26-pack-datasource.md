---
status: superseded by 2026-08-28-pack-datasource-toolkit.md
---

# PACK Datasource 与多数据源联合查询

> 本文已由 `2026-08-28-pack-datasource-toolkit.md` 取代，只保留 Provider facade 方案的设计历史，不得作为实现合同。

## 1. 目标

用一个比现有多 Source 融合方案更小的模型支持数据源扩展：

- PACK 内的 Datasource 独占数据源定义、参数、解析、来源内查询与物化语义；
- Datasource 可以面向本地文件、本地数据库、远程数据库或其他来源；
- KAT 平台不理解具体来源的连接、解码、表发现或下推规则；
- 多个 Datasource 仍能在同一次 Query/Workflow 中联合查询；
- DataFusion 继续只存在于 Python 层：本地 Source executor 可以拥有私有 Session，多源融合 Session 由 Workflow Runtime 拥有。

本文记录已经逐项确认的目标方案，取代 `2026-08-25-multi-source-fusion.md` 与 `2026-08-25-postgresql-federated-source.md` 中对应设计。`accepted` 表示设计边界已经闭合，不表示代码已经交付；实现仍按 D50 的两个 PR 和本文验收矩阵推进。

## 2. 一页全貌

核心模型只有一句话：**PACK Datasource 负责把来源查询变成一张 Arrow/Parquet 表，KAT Provider facade 负责把它安全落成本地 `Table`，Workflow 只在需要跨来源时用 `ctx.sql()` 联合这些本地 Table。**

| 角色 | 所有者 | 负责 | 不负责 |
|---|---|---|---|
| Datasource | PACK | factory、配置解释、解析、来源内物化、选择 Source executor | 平台注册、Run Output 发布、跨源规划 |
| Source executor | PACK | 来源方言、连接、私有 catalog、`execute()` 生成一个 Arrow stream 或 `ParquetSource` | 结果命名、Runtime 输出路径、融合 Session |
| Provider facade | KAT | `query()`、名称保留、partial、默认 Parquet、本地自动注册、`Table` 构造 | 理解来源 SQL、发现远端表、透明下推 |
| Table | KAT | 表示一张已经完整本地化、不可变且具名的关系 | 远端 cursor、延迟查询计划、整表驻留内存 |
| Workflow Context | KAT | `ctx.provider()`、`ctx.sql()`、`ctx.from_arrow()`、`ctx.datasource_root` | 全局 Provider registry、Binding、Datasource 身份 |

```text
PACK Workflow
  ├─ Datasource A factory ─> ctx.provider(executor A) ─> Provider A
  │                              └─ query(source SQL, name="a")
  │                                   └─ outputs/a.parquet + Table a + 自动注册 a
  ├─ Datasource B factory ─> ctx.provider(executor B) ─> Provider B
  │                              └─ query(source SQL, name="b")
  │                                   └─ outputs/b.parquet + Table b + 自动注册 b
  └─ ctx.sql("... FROM a JOIN b ...")
       └─ 本地 DataFusion DataFrame ─> Run Output
```

目标领域模型不新增或依赖 Federation planner、远端 TableProvider、Binding、平台 Dataset、`ctx.register()` 或隐式 Provider 查找。来源查询始终由对应 executor 完整执行；融合查询始终只读已经本地化的关系。首个里程碑尚未删除旧 Dataset 实现，D51 只定义迁移期兼容输入，不把 Dataset 带回目标模型。

## 3. PACK 作者工作流

### 3.1 单来源

只查询一个来源时，不需要 `ctx.sql()`：

```python
pg = postgresql.provider(
    ctx,
    profile=profile,
    database=database,
)

return pg.query(
    "SELECT thread_id, cpu_usage FROM observation WHERE observed_at >= $1",
    params=(start_ns,),
    name="observations",
)
```

`query()` 返回前已经执行远端 SQL、流式写入 Parquet、关闭连接并形成 `Table`。直接返回该 Table 时，Run Output 名就是 `observations`，不会再次执行远端 SQL。

### 3.2 多来源

跨来源时，先分别查询并命名，再写普通 DataFusion SQL：

```python
pg.query("SELECT ...", name="telemetry")
local.query("SELECT ...", name="switches")

return ctx.sql("""
    SELECT *
    FROM telemetry
    JOIN switches USING (thread_id)
""")
```

调用顺序就是执行顺序。每次 `query()` 成功后，结果名已经自动进入当前 operation catalog；SQL 中不存在的表直接报错，不会触发来源查询或 Provider fallback。

### 3.3 两类 SQL 不混用

- **Source query**：传给 `Provider.query()`，方言、占位符、参数和下推全部属于该 Datasource。例如 PostgreSQL 使用 `$1` 和位置参数，本地 DataFusion executor 使用 `$name` 和 Mapping。
- **Fusion query**：传给 `ctx.sql()`，只使用当前 operation 已注册的本地关系，采用现有 DataFusion 方言与参数规则。

同一个 PostgreSQL Database 内的 Join、Filter、Aggregate 应写在同一条 Source query 中并由远端执行。不同 Database 或不同来源之间的 Join 必须先各自产生本地 Table，再由 `ctx.sql()` 完成。D49 给出一个 service、两个 Database 和本地 Parquet 的完整可运行案例。

## 4. 最小公共接口与生命周期

PACK 作者只需要以下新接口：

```python
from pathlib import Path
from typing import ContextManager, Protocol

import kat
import pyarrow


class SourceExecutor(Protocol):
    def execute(
        self,
        sql: str,
        params: object | None,
        *,
        scratch: Path,
    ) -> ContextManager[pyarrow.RecordBatchReader | kat.ParquetSource]: ...

    def close(self) -> None: ...


source = kat.ParquetSource(path)
provider: kat.Provider = ctx.provider(executor)
table: kat.Table = provider.query(sql, params=None, name=None)
table.name       # str，只读
table.schema     # pyarrow.Schema，只读
```

`kat.SourceExecutor`、`kat.ParquetSource`、`kat.Provider` 与 `kat.Table` 都从现有顶层 Pack Authoring API 导出。`ParquetSource(path: Path)` 是 PACK 作者可构造的不可变单字段值对象，只公开只读 `path`；Runtime 再 canonicalize 并按 D37 验证、转移或复制。`Table` 只能由 KAT Provider facade 创建，只公开只读 `name` 与 `schema`，不公开 backing path、Runtime workspace 或可替换 executor。`Provider` 的类型可用于标注，但没有面向 PACK 的公共构造器或继承点。

一次成功 query 的固定顺序是：

```text
校验并保留 name
  -> 分配 query scratch 与私有 partial
  -> 进入 executor context
  -> 完整消费 RecordBatchReader / 接收 ParquetSource
  -> sink 完成并关闭
  -> 正常退出 executor context
  -> partial finalize 为 outputs/<name>.parquet
  -> 自动注册 name
  -> 返回 Table
```

在 `Table` 返回前任一步失败，当前 Context 都不可发布；即使 PACK 捕获异常，也不能继续查询或发布成功 Run。成功形成的 Table 不再访问来源。Workflow 结束后的 `executor.close()` 与不可见中间文件删除是尽力清理，失败只记录 warning，不改变已经形成的 Output 正确性。

所有生产产物位于 `KAT_DATA_HOME`：Runtime 管理当前候选 Run 的 `outputs/` 与临时文件；Datasource 若需要跨 Workflow 物化，只能使用当前 PACK 的 `ctx.datasource_root == KAT_DATA_HOME/datasources/<pack-name>/`，其内部布局和重用规则仍由 Datasource 自己定义。

## 5. 详细决策记录

### D1：Datasource 拥有来源内行为，Workflow Runtime 拥有跨源组合

Datasource 独占：

- 来源参数与配置；
- 数据解析与来源事实定义；
- 表或关系发现；
- Source executor 的 SQL 方言、查询与执行策略；
- 物化能力与来源特定的选择规则；
- 来源类型到 Arrow/Parquet 的转换；
- 来源资源的创建、关闭实现与来源侧取消机制。

Python Workflow Runtime 独占：

- Provider 查询结果的默认 Parquet 落盘；
- 多源融合使用的本地 DataFusion Session；
- 查询结果名称分配、落盘和 operation-local 自动注册；
- 查询级资源关闭编排与 operation 结束时的 executor 关闭；
- 跨 Datasource 结果的 Join、Aggregate 与其他关系组合；
- Workflow 执行和 Run Output 发布。

Provider 是统一的来源查询面。一次 `query()` 接收一条来源内 SQL，立即执行来源查询并返回一张已经本地化的单表 `Table`：

```python
table = provider.query(sql, params=...)
```

`Table` 不是整表内存对象、查询计划或仍然打开的 cursor。Source executor 产生 Arrow batch stream 或已有 `ParquetSource`；Runtime 的通用 sink 在 `query()` 返回前完成默认 Parquet 落盘并关闭查询级资源。返回的 `Table` 是指向完整、不可变本地结果的关系句柄。

若 Workflow 直接返回单个 Provider 的 `Table`，Run Output 发布直接采用这份已经写在候选 Run Output 目录中的 Parquet，不重新执行来源 SQL，也不解码后重写。`Provider.query(..., name=...)` 完成落盘后会自动把同名关系注册到当前 operation catalog；多个 `Table` 需要联合时，Workflow 直接用普通 `ctx.sql(...)` 引用这些名字。

Datasource 独占来源内查询语言和执行，例如 PostgreSQL SQL；`ctx.sql()` 只解析显式输入 `Table` 之上的 DataFusion 融合 SQL。KAT 不拆分一条跨源 SQL，也不要求远端 Provider 暴露原始表、TableProvider 或 DataFusion Session。

### D2：Workflow 直接组合 Provider 查询结果

Workflow 把当前 `ctx` 显式传给 Datasource factory，创建与本次 operation 绑定的 Provider，再直接调用其 `query()`。KAT 不持久化 Datasource Binding，也不在 Dataset 中维护 External/Materialized 切换状态。

单数据源 Workflow 直接返回 Provider 查询结果：

```python
@kat.workflow(...)
def observations(ctx: kat.Context, pg_profile: str, start: str):
    pg = postgresql.provider(
        ctx,
        profile=pg_profile,
        database="telemetry",
    )
    return pg.query(
        """
        SELECT process_id, thread_id, cpu_usage
        FROM observation
        WHERE observed_at >= $1
        """,
        params=(start,),
    )
```

多数据源 Workflow 为每个需要在融合 SQL 中引用的查询结果指定名称，再直接调用 `ctx.sql()`：

```python
@kat.workflow(...)
def analyze(ctx: kat.Context, trace_root: str, pg_profile: str, start: str):
    telemetry = postgresql.provider(
        ctx,
        profile=pg_profile,
        database="telemetry",
    ).query(
        """
        SELECT thread_id, cpu_usage
        FROM observation
        WHERE observed_at >= $1
        """,
        params=(start,),
        name="telemetry",
    )

    # `trace_parquet` 是这个 PACK 自己定义的 Datasource，不是 KAT 内建布局。
    switches = trace_parquet.provider(ctx, root=trace_root).query(
        """
        SELECT next_thread_id, timestamp
        FROM sched_switch
        """,
        name="switches",
    )

    return ctx.sql("""
        SELECT t.*, s.timestamp
        FROM telemetry AS t
        JOIN switches AS s
          ON t.thread_id = s.next_thread_id
    """)
```

Datasource-owned Source executor 拥有来源 SQL、解析、连接、缓存、内部物化与类型转换流程。`query()` 调用期间，executor 把结果交给 Runtime 的通用 sink；`Table` 的命名、默认落盘、自动注册与资源关闭编排由 KAT facade 完成，executor 不接收本地 `name`、融合 Session 或 Runtime landing path。`query()` 只有在来源查询、本地化、自动注册和查询级资源关闭都成功后才返回。

该决策同时取消以下通用平台面：

- 脱离 Workflow 独立重开数据源的 Datasource Binding；
- 通用 `kat query --dataset` 旧 Dataset 查询入口；
- 通用 `kat materialize --dataset --source ...`；
- 由 KAT 统一定义的 External/Materialized shadowing。

来源查询与物化只能由 Workflow 触发，或由 Datasource/Provider 自身的明确能力触发。`kat query --run` 对已发布 Run Output 的后续查询不受影响。

### D3：Workflow 代码固定可使用的 Provider 集合

Workflow 代码明确选择 Datasource Provider，或明确列出用户可选的有限 Provider 实现。用户可以提供这些 Provider 的配置，但不能在运行时传入任意 Python Provider 类、module 路径或可执行代码。

```python
@kat.workflow(...)
def analyze(ctx, mode, pg_profile, trace_path):
    if mode == "online":
        telemetry = postgresql.provider(
            ctx,
            profile=pg_profile,
            database="telemetry",
        ).query(
            """
            SELECT process_id, thread_id, cpu_usage
            FROM observation
            """,
            name="telemetry",
        )
    else:
        telemetry = telemetry_parquet.provider(ctx, root="./telemetry").query(
            """
            SELECT process_id, thread_id, cpu_usage
            FROM observations
            """,
            name="telemetry",
        )

    trace = hitrace.provider(ctx, path=trace_path).query(
        """
        SELECT next_thread_id, timestamp
        FROM sched_switch
        """,
        name="trace",
    )
    return ctx.sql("""...""")
```

这使 Provider 保持可配置，同时让 Workflow 明确拥有每个来源 SQL、查询结果名、来源语义和测试范围。来源 SQL 不要求跨 Provider 可移植；运行时选择仍限于代码列出的实现，不会变成任意 Provider 注入。

### D4：Provider 配置通过 Workflow inputs 进入，凭据由 Provider 外部解析

影响数据身份且可安全记录的 Provider 配置作为 Workflow inputs，例如本地文件路径、非敏感 PostgreSQL libpq service 名和 Database 名。它们沿用 Workflow 现有类型解析与 effective input 记录，随成功 Run 进入 Run Manifest。PostgreSQL factory 的 `profile` 参数就是该 service 名，不指向 KAT 自建的 profile 对象或配置仓库。

凭据不是 Workflow input。PostgreSQL Datasource 通过 libpq service file、password file 及所属环境的外部凭据机制取得实际连接参数；其他 Provider 使用自己的生态。KAT 不建立通用 Provider 配置仓库或凭据仓库。

复杂的非敏感配置优先以一个来源生态已有的 profile/service selector 或配置文件路径进入，其格式与解析属于 Provider；不把 Provider 的每个底层开关提升为 KAT 公共配置。

### D5：目标模型不再使用平台级 Dataset

目标迁移完成后，平台级 Dataset 不再承担生产 Workflow 的输入容器。`kat run` 只接收 Workflow inputs，Run Manifest 不再记录 Dataset 路径，Output Query 也不重新打开当时的来源。首个里程碑期间的旧输入兼容由 D51 单独约束，不形成新 Datasource API。

Datasource 若要保留本地可重用结果，可以在当前 PACK 的 `ctx.datasource_root` 下生成来源特定的文件或目录。后续 Workflow 通过普通 input 传入由该 Datasource 解释的 artifact key 或其他选择值；目录结构、表格式、完整性、版本和重用规则都属于该 Datasource。它只是普通的 Datasource 物化产物，不是具有 KAT 身份的平台实体。

```python
if mode == "online":
    telemetry = postgresql.provider(
        ctx,
        profile=profile,
        database=database,
    ).query("SELECT thread_id, cpu_usage FROM observation")
else:
    telemetry = telemetry_parquet.provider(ctx, root=artifact_path).query(
        "SELECT thread_id, cpu_usage FROM observations"
    )
```

PACK 测试使用 Provider fixture 创建所需 Provider，不依赖平台 Test Dataset。

### D6：目标模型删除 Workflow `required_tables`

Provider 由 Workflow 函数内部选择并查询，KAT 在调用 Workflow 前无法也不需要预先枚举其内部关系。目标迁移完成后，Workflow 不再维护与来源 SQL 重复的 `required_tables` 清单；首个里程碑中的新 example 暂时声明空列表，以符合尚未删除的现有 decorator schema。

```python
@kat.workflow(name="analyze", ...)
def analyze(ctx, ...):
    telemetry = postgresql.provider(ctx, ...).query(
        "SELECT thread_id, cpu_usage FROM observation",
        name="telemetry",
    )
    trace = hitrace.provider(ctx, ...).query(
        "SELECT next_thread_id FROM sched_switch",
        name="trace",
    )
    return ctx.sql("""...""")
```

`Provider.query()` 是同步执行边界，因此 Provider 不可用、来源 SQL/解析失败、来源类型无法转换为 Arrow、名称冲突或默认落盘失败，都在对应 `query()` 调用中发生。融合输入缺失、融合 SQL 与结果 Schema 不匹配，则在 `ctx.sql()` 的规划或执行阶段失败。Workflow 的封闭 Provider 集合、固定来源查询与融合查询，以及 Provider fixture 测试，是它的正确性证据。

### D7：一次 Provider 查询产生一张自动注册的具名表

Source executor 的内部 catalog 可以包含任意多张表，但一次 facade `query()` 对外只产生一张 `Table`。KAT facade 为结果分配当前 operation 内唯一的扁平名称，以该名称写入 Parquet、设置 `Table.name` 并自动注册到本地 DataFusion；后续 `ctx.sql()` 直接使用该名称。

```python
telemetry = postgresql.provider(
    ctx,
    profile=profile,
    database="telemetry",
).query("""
    SELECT process_id, thread_id, cpu_usage
    FROM observation
""", name="telemetry")

control = postgresql.provider(
    ctx,
    profile=profile,
    database="control",
).query("""
    SELECT process_id, process_name
    FROM process_registry
""", name="control")

trace = hitrace.provider(ctx, path=trace_path).query("""
    SELECT next_thread_id
    FROM sched_switch
""", name="trace")

return ctx.sql("""
    SELECT t.*, c.process_name
    FROM telemetry AS t
    JOIN control AS c USING (process_id)
    JOIN trace AS s
      ON t.thread_id = s.next_thread_id
""")
```

PostgreSQL 多 Database 不进入平台身份模型；Workflow 为每个 Database 创建 Provider 并显式定义来源 SQL，再在融合调用中给结果命名。多个 Provider 能否在 Datasource 内部共享 service 配置、连接池或缓存，不属于 KAT 合同。

KAT 在执行来源查询前验证或生成名称，并在当前 operation 中原子保留该名称；重名立即失败，不提供覆盖或隐式复用语义。融合 SQL 不重复 PACK、Datasource、Database 或远端 schema 身份，也不持久化本次 operation 的查询结果名。融合输入是来源查询产生的结果表，不是对远端原始表命名空间的透明映射。

### D8：Datasource 是 PACK 内的普通 Python 模块

Datasource 只是被 Workflow import 的 Python 模块或工厂。一个 PACK 可以按需要定义零个、一个或多个 Datasource 模块；模块路径、拆分方式和命名都是 PACK 的代码组织，不是 KAT 平台身份。Provider factory 的唯一通用要求是显式接收当前 `ctx`，用它构造 operation-bound 的 KAT Provider facade。

```python
from kat.pack.helpers.datasources import postgresql

@kat.workflow(name="analyze", ...)
def analyze(ctx, database, ...):
    return postgresql.provider(ctx, database=database).query(
        "SELECT thread_id, cpu_usage FROM observation"
    )
```

KAT 不提供 `@kat.datasource`、Datasource 注册表、运行时发现或独立检查命令。KAT 与来源代码的稳定边界是 factory 返回的 operation-bound Provider facade 及其 `Table`；factory、来源 executor、内部 catalog 和目录布局仍是普通 PACK Python 代码。

### D9：物化是 Datasource 的可选能力，不属于统一 Provider contract

KAT 不定义 `provider.materialize()`，也不要求所有 Provider 支持长期物化。`Provider.query()` 可以解析来源、读取或更新不改变查询语义的 Datasource 私有缓存，但不能借查询配置创建一个由用户命名、需要独立生命周期合同的长期物化。具名且跨 Workflow 复用的物化必须由 Datasource 显式函数或专用 Workflow 发起。

需要跨 Workflow 复用时，物化结果只是 Datasource 在当前 PACK 的 `ctx.datasource_root` 内定义的文件或目录；后续 Workflow 通过普通 input 接收 artifact key 或其他来源特定选择值，再由 Datasource 解析具体路径。KAT 不为它分配 ID，不维护统一 Manifest，也不规定目录格式、有效期、版本、缓存命中或清理策略。

Provider 查询结果的临时/Output Parquet 落盘属于 Runtime；语义透明的内部 cache 与显式长期物化都属于 Datasource，但只有后者形成作者可选择的 artifact key。显式物化失败时错误会终止当前 Workflow，其产物完整性、恢复和重建策略由对应 Datasource 定义。

### D10：File Parser 是 Datasource 内部组件，native 解析采用一次性批处理边界

面向文件的 Datasource 可以组合 File Parser。KAT 可以随产品提供多个可复用 Parser，目标包括 Hitrace、独立 Ftrace 等，PACK 作者也可以提供自定义 Parser。File Parser 不是 Datasource Provider，不直接成为 Workflow 查询对象，也没有平台注册或持久身份。

当前仓库中只有 Hitrace 是完整文件解析入口；现有 `ftrace` 只是 `.htrace` 容器内 `ftrace-plugin` 的私有 decoder，不是 raw Ftrace 文件 Parser。只有另行交付独立文件 adapter 后，才能把 Ftrace 作为独立内建 File Parser 暴露。新 Hitrace Parser 以当前正式 `import_hitrace` 产生的表、Schema、时钟语义与完整性检查作为迁移基线，不采用旧的查询或 raw event 路径。

当 File Parser 使用 native binary 时，Python Datasource 每次解析只启动它一次：通过 JSON 文件传入输入路径、输出根目录和 Parser 配置；native parser 完整解析原始数据，把多个来源事实表写成 Parquet，并通过 JSON 文件返回表索引、相对路径或错误。这个多表根目录是 Provider 的内部 catalog，不直接成为 Fusion query 的输入集合。

```text
Python Datasource
  -> native File Parser（一次完整调用）
  -> 多表 Parquet 根目录 + JSON response
  -> Source executor 私有 DataFusion catalog
  -> Provider.query(SQL, name=...)
  -> 单表 Table
```

解析过程中不逐行跨 Python/Rust 调用，不跨进程传递 DataFrame、Logical Plan 或内存 Arrow buffer，native binary 也不链接 DataFusion。解析得到的多表根目录默认写入 Runtime 为当前 `query()` 分配的 scratch workspace；Source executor 在 Python 层用私有 DataFusion 查询这些表，再把单表结果交给 Runtime 通用 sink。Datasource 只有在自身配置要求复用时才把根目录发布到当前 PACK 的 `ctx.datasource_root` 下。

这个边界只约束选择 native binary 的文件解析实现。PostgreSQL executor 使用来源自身驱动；本地 Parquet/Arrow executor 使用私有 DataFusion；两者都通过 KAT facade 的 `query(..., name=...) -> Table` 对 Workflow 呈现同一形状。远程 executor 不向融合 DataFusion 暴露原始表或自定义 TableProvider。Workflow 代码固定实际采用的内建或自定义 Parser；Workflow input 不能注入任意 Parser module 路径或可执行代码。

### D11：自定义 File Parser 是代码扩展点，不是平台插件

PACK 作者可以在 Datasource 中直接 import 并调用自定义 File Parser。实现可以是纯 Python，也可以由 Python 包装 native binary；它不需要实现 KAT 统一的 `FileParser` class、注册 decorator、插件 Manifest 或发现协议。

KAT 可以提供调用 native binary、校验 JSON response 和读取多表索引的辅助库。选择使用该辅助库的 Parser 遵循相应文件协议；不使用该辅助库的 Datasource 可以采用其他内部实现，只要最终通过 Provider 查询返回 `Table`。

内建 File Parser 与 PACK 自定义 Parser 在模型上没有不同，前者只是 KAT 随产品提供的复用实现。Workflow/Datasource 代码明确选择实际允许的 Parser，运行时用户只能传入该代码已经声明的普通配置，不能通过 CLI 或 input 动态注入 Parser module、class 或 executable。

### D12：删除 `kat import`，文件来源由 Workflow 直接打开

目标用户面不再提供 `kat import`。该命令原本把原始文件解析成平台 Dataset；新模型既没有平台 Dataset，也不要求用户在运行 Workflow 前完成独立导入阶段。

文件路径和其他非敏感来源配置作为 Workflow inputs。Workflow 创建并查询对应 Provider；Datasource 在 `query()` 调用中按自身策略调用内建或自定义 File Parser、建立内部 catalog 并执行来源 SQL。解析失败作为当前 Workflow 失败向上传播。

```text
kat run
  -> Workflow inputs
  -> file Datasource / File Parser
  -> Provider 内部多表 catalog
  -> Provider.query(SQL, name=...) -> 自动落盘并注册 Table
  -> 可选 ctx.sql(...) 融合
  -> Run Output
```

Parser 默认使用的中间 Parquet 写入 Runtime 为当前 `query()` 提供的 scratch workspace，并随 operation 生命周期处理。需要跨 Workflow 复用时，由 Datasource 在当前 PACK 的 `ctx.datasource_root` 下选择长期物化位置；这不会恢复 `kat import`、Dataset inspection 或通用导入产物身份。

### D13：删除 `kat bind`，Provider 只由 Workflow 创建

目标用户面不提供 `kat bind`。KAT 不保存从逻辑 Source 到外部位置、连接参数或物化目录的持久 Binding，也不维护 External/Materialized 状态。

Workflow inputs 提供本次执行所需的非敏感 Provider 配置，Workflow 代码据此创建并查询 Provider。切换 PostgreSQL Database、服务 profile 或本地文件只需改变相应 input，不需要先修改 KAT 状态。

```python
@kat.workflow(name="analyze", ...)
def analyze(ctx, pg_profile, database, ...):
    return postgresql.provider(
        ctx,
        profile=pg_profile,
        database=database,
    ).query("SELECT thread_id, cpu_usage FROM observation")
```

这些非敏感 effective inputs 随成功 Run 记录；凭据继续由 Provider 从所属生态的外部环境解析。来源连接和查询资源只属于对应 `query()` 调用，返回 `Table` 后不再依赖远端连接。

### D14：删除通用 `kat materialize`，物化只能由 Datasource/Workflow 表达

目标用户面不提供通用 `kat materialize`。不同 Datasource 对选择关系、过滤范围、格式、增量、有效期、完整性和清理有不同语义，KAT 不建立能够假装统一这些差异的命令参数或执行合同。

Datasource 可以在 Provider 内部使用语义透明的私有缓存；具名长期物化则由 Workflow 明确调用来源特定函数。PACK 若需要用户显式发起物化，可以提供一个专用 Workflow；用户通过普通 Workflow input 提供 artifact key 或其他来源特定选择值，不直接传入平台内部绝对目标路径。

```python
materialized_root = (
    ctx.datasource_root
    / "postgresql"
    / materialization_key
)
tables = postgresql.materialize_snapshot(
    profile=pg_profile,
    database=database,
    destination=materialized_root,
)
return parquet.provider(ctx, tables=tables).query(
    "SELECT thread_id, cpu_usage FROM observation",
    name="telemetry",
)
```

KAT 只执行 Workflow 并传播失败，不理解 `materialize_snapshot()` 要复制哪些来源表，也不管理长期物化产物。上例中的 `materialized_root` 与随后 `query()` 由 Runtime sink 写入的候选 Output 是两个独立生命周期；后者若成为 Run Output，直接发布现有 backing，不重新访问 PostgreSQL。

### D15：新方案取代 PR #225 的 Source 生命周期模型

[PR #225](https://github.com/maokelong/kat-cli/pull/225) 不再作为新方案的合并基础，而作为已完成的原型、验证证据和可选择性提取的实现素材。[Issue #224](https://github.com/maokelong/kat-cli/issues/224) 中“让 PACK 拥有可扩展来源能力”的目标保持不变，但解决方案需要按本文重写。

保留的方向包括：

- PACK 拥有来源解释和分析能力；
- 只有 Workflow 是 `kat run` 的任务入口；
- 删除 `required_tables` 和旧的顶层 Data Import 流程；
- 复用 DataFusion、Arrow、Parquet、成熟数据库驱动与来源生态；
- Hitrace 的正式来源事实、Schema、时钟和完整性检查继续作为迁移基线。

明确不继承的机制包括：

- `@kat.source`、`sources/` 扫描和 Source inspection；
- Dataset 内的 External/Materialized Binding 与 REDO recipe；
- `kat bind`、`kat materialize` 和旧 Dataset 查询入口；
- `PACK catalog / Source schema / table` 持久名称模型；
- Workflow 之外的统一 Source Resolution；
- 让 Hitrace native parser 通过 DataFusion FFI 直接成为 Provider。

可复用代码必须按后续最小切片重新证明符合新边界，例如提取 Hitrace parser-focused native binary、复用 PostgreSQL Provider 探索、Payload 构建方式或测试 fixture；不为了保留 PR #225 的代码形状增加兼容层。PR #225 尚未形成正式发布的数据与 Binding 合同，因此不提供其 Dataset 布局或 Binding metadata 的迁移工具。

### D16：迁移完成后，旧 Data Import Dataset 不转换为新模型

首个里程碑按 D51 继续识别旧 Dataset 输入，使现有 Workflow 能在迁移前正常运行；这只是旧实现共存，不是把它转换成 Datasource。后续独立迁移切片删除旧 Dataset 能力时，不自动转换或删除旧 `kat import` 目录。删除完成后，这些目录原样保留，但只被视为用户控制的普通文件目录，不再提供 Dataset inspection、旧 Dataset 查询入口或作为 `kat run` 输入的能力。

仍有原始文件时，用户通过新 Workflow input 传入原始位置，由 Datasource 重新解析。只剩旧 Parquet 时，PACK 可以明确使用普通 Parquet Datasource 读取所需文件；KAT 不识别旧 marker、catalog、表布局或其他 Dataset metadata，也不提供将其整体包装成新来源的兼容 Adapter。

这项最终迁移策略不承诺删除旧实现后的 Dataset 继续可用，也不主动销毁数据。需要继续使用旧平台语义时只能保留旧版本工具；新模型的验证只覆盖从原始来源或显式普通 Parquet 输入开始的流程。

### D17：Provider 的统一公共形状是 `query(SQL, name=...) -> Table`

面向 Workflow 的 Provider 是 KAT 拥有的 operation-bound facade，不是来源作者任意实现的一组同名方法。其最小概念合同为：

```python
class Provider:
    def query(
        self,
        sql: str,
        *,
        params: object | None = None,
        name: str | None = None,
    ) -> Table: ...
```

一次 `query()` 只返回一张 `Table`，不返回多 result set、catalog 或多表 Mapping。Datasource-owned executor 内部可以有任意多张来源表；Workflow 通过来源 SQL 选择、Join 或聚合后得到一个结果。SQL 方言和 `params` 的具体形状属于来源 executor，KAT 不解析来源 SQL，也不承诺不同 Provider 之间 SQL 可移植。`name` 是 KAT facade 消费的本地结果名，不传给来源执行器。

`query()` 是 eager、同步的来源执行边界。它在调用期间完成来源 I/O、解析或来源内 SQL，并把结果交给 Runtime 的通用单表 sink。Runtime 按 batch 消费 Arrow 结果并默认写成候选 `outputs/<name>.parquet`；已有稳定 Parquet 按 D37 转移所有权或复制成独立 backing，避免解码后重新编码。只有完整结果已经可读、Schema 已确定且本次查询的 cursor、reader、parser 任务等 query-local 资源已释放后，`query()` 才返回 `Table`。连接池或私有 DataFusion Session 等 operation-scoped 资源可以供同一 Provider 的多次查询复用，在 operation 结束时由 facade 关闭 executor。

Source executor 可以自定义解析、缓存、查询、类型转换和内部物化过程，但不知道 Runtime 的 landing path，也不接触融合 DataFusion Session。它通过 D33 的 context-managed `pyarrow.RecordBatchReader | ParquetSource` 端口交付结果；不能通过让 executor 自己选择候选 Output 路径来绕开该边界。

返回后的 `Table` 是不可变、本地且可重复读取的具名单表关系。KAT 已经用 `Table.name` 自动将其注册到本地 DataFusion；它可以直接作为 Workflow Output，也可以由 `ctx.sql()` 按名读取，后续读取不会再次执行来源 SQL。融合路径中的 `ctx.sql()` 保持现有行为，返回本地 DataFusion `DataFrame`，而不是新的 KAT `Table`。

已有 Parquet 作为来源输入时可以位于 `KAT_DATA_HOME` 之外。提供方必须保证它在本次 `query()` 期间保持可读且内容不变；查询结果仍由 Runtime 以 `Table.name` 带入候选 Run Output 目录。Runtime 可以使用产生独立内容的 copy-on-write clone，不能使用 hard link 或 symbolic link；平台不支持 clone 时退化为字节复制，因此不承诺零拷贝。

### D18：PostgreSQL 来源查询由 Workflow 显式配置

PostgreSQL Datasource factory 用明确的连接目标创建 Provider facade。Workflow 每次调用 `query()` 都立即提交并完整执行一条 PostgreSQL SQL；结果本地化后成为一张 `Table`。KAT 不发现、枚举或注册远端原始表。

```python
telemetry = postgresql.provider(
    ctx,
    profile=profile,
    database=database,
).query(
    """
        SELECT p.process_name, o.thread_id, o.cpu_usage
        FROM observation AS o
        JOIN process_registry AS p USING (process_id)
        WHERE o.observed_at >= $1
    """,
    params=(start,),
)
```

调用 facade `query()` 时，PostgreSQL executor 在远端执行 SQL 并把结果转换成批式表数据，Runtime 通用 sink 同步完成默认本地化；远端 relation 不存在、权限不足、PostgreSQL 方言错误、类型无法转换或落盘失败时，当前 Workflow 立即失败。executor 不需要知道结果最终是直接 Run Output，还是多源融合输入。

同一 Database 内需要联合的表应在同一条来源 SQL 中完成 Join、Filter、Aggregate 等工作。哪些关系可读由 Source executor 使用的 PostgreSQL 身份和数据库权限决定。连接池、事务、查询缓存以及同一 Provider 多次查询的一致性策略不形成首版 KAT 公共合同。

### D19：查询显式分为来源查询和本地融合查询

KAT 不接收一条引用多个来源原始表的统一 SQL，也不自动把 SQL 拆成远端与本地计划。单数据源 Workflow 只有来源查询；需要多源融合时，Workflow 才显式编写第二阶段：

1. **来源查询**：每个 Provider facade 把 SQL 交给对应 Source executor 同步完成查询，并经 Runtime 通用 sink 返回一张已经本地化、自动注册的具名 `Table`。PostgreSQL SQL 完整提交给 PostgreSQL；本地多表根目录由对应 Source executor 的私有 DataFusion 查询。
2. **融合查询**：Workflow 直接用 `ctx.sql(sql)` 按 `Table.name` 引用这些本地结果，并返回现有 DataFusion `DataFrame`。

```text
PostgreSQL A.query(SQL, name="a") ──> Runtime sink ──> 本地 Table a ──┐
                                                                      ├──> 自动注册 ──> ctx.sql() ──> DataFrame
本地 Provider.query(SQL, name="b") ─> Runtime sink ──> 本地 Table b ──┘
```

因此“同库 SQL 必须下推”不需要框架下推器：Workflow 直接把同库 Join、Filter、Aggregate、Sort 与 Limit 写进该 Provider 的来源 SQL，它们天然在远端完成。不同 Database 各自执行来源 SQL 并返回结果，再由本地 DataFusion 联合；KAT 不把一个 Provider 的数据上传到另一个来源。

Workflow 作者明确控制每个来源结果的列、过滤条件、聚合粒度和时间窗口，也承担避免把无界大结果拉回本地的责任。首版不定义成本估算、通用传输预算、自动物化或跨源优化器；验证要同时检查来源 SQL 的实际执行和最终融合结果。

`ctx.sql()` 不能引用 `telemetry.public.observation` 等远端原始表；它只能引用当前 operation 中已经由成功 `Provider.query()` 自动注册的结果表名。不存在的关系直接按 DataFusion 的 table-not-found 错误失败，不触发 Provider 查找、远端发现或其他隐式回退。来源 SQL 和融合 SQL 的参数、方言与错误边界分别属于 Source executor 和 DataFusion，不混为一套隐式语义。

### D20：所有生产 Provider 产物位于 `KAT_DATA_HOME`

原始输入文件和远程来源可以位于 `KAT_DATA_HOME` 之外，但 KAT 与 Provider 在生产 operation 中生成的 JSON、Parquet、缓存和物化目录都必须位于当前选定的 `KAT_DATA_HOME`。

生产 `kat run` 已有 CLI 预分配的 `KAT_DATA_HOME/runs/<candidate-id>/`。每次 Provider 查询在来源 I/O 前先确定并保留结果名，然后把结果写入候选 Run 的 Output 目录：

```text
KAT_DATA_HOME/
  runs/<candidate-id>/
    outputs/
      <name>.parquet
    .scratch/
      <name>/
```

Runtime 拥有 `outputs/<name>.parquet` 的路径；Source executor 只获得独立的 scratch 能力，供 parser、驱动或私有 DataFusion 使用，并把 Arrow batches 或已有 `ParquetSource` 交给 Runtime sink，因此 executor 不感知 landing path。query scratch 内的 Parquet 可以转移给 Runtime；scratch 外的 Parquet 必须 clone 成独立内容或字节复制，不要求解码后重新编码，也不使用 hard link 或 symbolic link。KAT 不把该约束伪装成不受信任代码沙箱；native parser helper 仍要机械拒绝绝对输出路径和逃逸 scratch workspace 的相对路径。

backing 位于候选 `outputs/` 不等于它已经成为 Run Output；Run Manifest 仍是唯一发布事实。Workflow 直接返回的 `Table` 以 `Table.name` 发布其现有 backing，不重新执行 SQL或重编码；仅作为 `ctx.sql()` 输入、没有直接返回的 Provider backing 不进入 Manifest，并在所有惰性 DataFrame Output 完成后尽力清理。若声明的 Output 已全部成功，清理失败只写 Operation log warning，不阻止 Run 发布。query-local 来源资源在 `query()` 返回前已经释放，融合 DataFusion 始终只读取本地 Parquet；operation-scoped executor 资源在当前 operation 结束时释放。`kat test` 使用每次测试执行的 `pytest tmp_path` 对应 workspace，不写入生产 Run 目录。

需要跨 Run 保留的 Datasource materialization 必须位于 `KAT_DATA_HOME/datasources/<pack-name>/`，由 `ctx.datasource_root` 暴露给当前 PACK；其更深层子目录、命名和生命周期仍由 Datasource 定义。KAT 只提供 PACK-scoped 存储根，不恢复 Artifact ID、registry、统一 Manifest 或查询入口。

### D21：不引入远端 TableProvider 或 Federation 层

远程数据库和本地非 Parquet/Arrow executor 都不需要向融合 DataFusion 注册自定义 TableProvider。它们在 Source executor 内完成来源查询或解析，再由 KAT facade 返回 `Table`。本地 Parquet/Arrow executor 可以在内部使用自己的 DataFusion Session 查询多表根目录，但不把该内部 catalog 注册到融合 Session。

因此本方案删除 `compute_context`、Federation optimizer/planner、远端方言翻译、透明 pushdown 和 Provider identity 跨 FFI 的 go/no-go spike。远程 Datasource 可以内部复用成熟数据库驱动、ADBC 或 Arrow 能力，但对 Workflow 的输出边界只有 `Table`。以后若确有“让一条 `ctx.sql` 透明引用远端原始表”的需求，应另开 issue 重新证明复杂度，不在本方案预留半套 Federation Interface。

### D22：来源 SQL 的参数语义属于 Provider

`Provider.query(sql, params=..., name=...)` 把 SQL 和参数交给具体来源执行器；`name` 只由 KAT 通用包装层消费。KAT 不统一来源 SQL 的占位符，不解析或改写 SQL，也不把一种 Provider 的参数格式翻译成另一种格式。

PostgreSQL executor 首版只接受 `None` 或一组位置参数值，并使用 PostgreSQL 原生 `$1`、`$2` 占位符；不接受 `%s`、具名参数、Arrow batch 或 `executemany`。Python 参数值原样交给 ADBC 的单组参数绑定，不由 KAT facade 格式化、拼接或做来源类型转换。

本地 Parquet/Arrow executor 只接受 `None` 或 `Mapping[str, scalar]`，SQL 使用 DataFusion `$name` 占位符，标量允许集合与转换规则直接复用现有 `ctx.sql()`；它不接受位置参数。两种 executor 都不允许参数替换 identifier 或任意 SQL 片段；需要动态选择表、列或查询形状时，由 Workflow 在已准入的固定代码分支中选择完整 SQL。

Fusion query 不继承 Source executor 的参数规则。`ctx.sql()` 继续使用 KAT 当前基于 DataFusion 的 `$name` 具名值参数，只绑定受支持的值类型，不替换 identifier 或 SQL 文本。

### D23：一次 Provider 查询是一个只读 command 和一张结果表

`Provider.query()` 每次只接受一个来源内只读 command，并返回一张具有明确 Schema 与 `Table.name` 的 `Table`。结果可以为零行；不支持多个 command、多个 result set 或没有表结果的 command。

KAT 不可能统一解析所有来源方言，因此只读边界由具体 Source executor 负责。PostgreSQL executor 的精确承诺由 D43 定义：服务端阻止 PostgreSQL 持久写入，并通过 prepare/bind/result-stream 路径限制为一个表结果；它不是任意函数或不可信 SQL 的外部副作用沙箱。其他 executor 必须按自己的来源能力建立等价的单结果只读边界。executor 可以在内部完成解析、缓存、临时文件和类型转换等不改变外部业务事实的准备工作。

长期物化、数据写入或其他有外部副作用的能力不伪装成 `query()`。Datasource 可以提供来源特定方法，PACK 也可以提供专用 Workflow；这些能力的参数、权限、幂等性和失败恢复仍由对应 Datasource 定义。

### D24：同一已本地化 Table 可以重复消费而不重复查询

`Provider.query()` 返回时，来源 SQL 已经执行一次并产生不可变的本地 backing。同一个 `Table` 被多个 Fusion query 或多个 Workflow Output 引用时，所有消费者直接复用该 backing；Runtime 不再执行来源 SQL，也不需要按对象身份维护延迟求值缓存。

```python
events = pg.query(
    "SELECT process_id, cpu_usage FROM observation",
    name="events",
)

summary = ctx.sql(
    "SELECT process_id, SUM(cpu_usage) FROM events GROUP BY process_id",
)
details = ctx.sql(
    "SELECT * FROM events WHERE cpu_usage > $minimum",
    minimum=10,
)

return {"summary": summary, "details": details}
```

上例的 PostgreSQL 来源 SQL 在创建 `events` 时执行一次，两个 DataFrame 都读取同一份本地结果。该候选 Output backing 只在当前 Workflow operation 内有效，不作为跨 Run 查询缓存。两次缺省命名的 `pg.query(same_sql)` 会得到相同 SQL hash 名，第二次在来源 I/O 前因重名失败；若显式提供两个不同 `name`，则分别执行两次来源 SQL并生成两张不同的 `Table`。KAT 不按 SQL 文本、Provider 配置或来源地址隐式复用结果。

若一张来源 `Table` 作为 Run Output 使用，Runtime 直接发布候选 `outputs/<Table.name>.parquet` backing。若它还有本地查询消费者，则在所有 DataFrame Output 完成前保持 backing 可读；无论哪种情况都不解码后重写整表。

### D25：首版 Provider 查询按 Workflow 调用顺序同步执行

`Provider.query()` 是普通的同步 Python 调用，其在 Workflow 函数中的调用顺序就是来源访问顺序。每次调用只有在名称保留、来源查询、本地化、自动注册和资源关闭全部完成后才进入下一条 Python 语句。即使返回的 `Table` 最终没有被 `ctx.sql()` 引用或作为 Output 返回，该查询也已经执行；KAT 不做 Output 依赖裁剪。

若某次 `query()` 失败，当前 Workflow Context 立即进入不可发布状态。即使 PACK 代码捕获该 Python 异常，后续 `ctx.sql()`、Provider 查询与 Output 发布也必须失败；KAT 不提供 Workflow 级来源重试或备用 Provider 切换状态机。`ctx.sql()` 只处理已经本地化并自动注册的结果，不触发或重试来源查询；结果未被 SQL 引用也不会撤销此前已经发生的来源访问。

Runtime 首版不自动并行调用多个 Provider。这使连接数量、临时磁盘占用、错误顺序和日志保持确定。Source executor 可以在单次查询内部自行并行解析、读取或执行；该行为属于 Datasource。需要 Workflow 跨 Provider 并行时，必须在真实延迟证据出现后另行设计资源预算、取消和错误聚合。

### D26：保留现有 `ctx.from_arrow()` 与 DataFrame 路径

现有 `ctx.from_arrow(pyarrow.Table) -> datafusion.DataFrame` 的参数类型、返回类型、执行 Session 和生命周期语义保持不变。它仍用于 Workflow 把已经位于内存中的 `pyarrow.Table` 转成当前 DataFusion DataFrame；不扩展为 batch stream、cursor、Parquet 路径或 Provider 查询入口。

新模型不增加公开的 `Table.from_arrow()`，也不复用 `ctx.from_arrow()` 承载 Provider 大结果。Provider 在 `query()` 内通过独立的内部 batch/Parquet sink 交付大结果，返回值只引用已经完成的本地 backing；具体内部接口另行确认。

为了保持既有行为，`ctx.sql()` 与 `ctx.from_arrow()` 都继续返回当前 Session 的 DataFusion `DataFrame`。单个 DataFrame 继续发布为 `main`，DataFrame Mapping 继续使用 key 命名。新 `Table` Output 是在此基础上增加的返回形态，以 `Table.name` 命名；Runtime 最终仍把两条路径发布成相同的 Run Output Parquet。

### D27：Provider 查询结果自动注册，删除 `ctx.register()`

融合 API 不增加 `ctx.fuse()`、`ctx.register()`，也不在 `ctx.sql()` 上传入 `tables`、`inputs` 或其他表 Mapping。Workflow 在 Provider 查询时给结果命名，成功返回后即可直接在 `ctx.sql()` 中引用：

```python
left = provider_a.query("...", name="left")
right = provider_b.query("...", name="right")

return ctx.sql("""
    SELECT *
    FROM left
    JOIN right USING (id)
""")
```

KAT facade 在来源执行前保留结果名，在完整落盘并关闭 query-local 来源资源后把 Parquet 自动注册到当前 Workflow operation 的 DataFusion catalog，然后才返回 `Table`。Source executor 只接收 SQL、来源参数和受控 scratch；本地 `name`、候选 Output 路径和 Session 注册均由 facade 处理。catalog 不跨 operation、Run 或 Workflow 持久化。

`ctx.sql()` 只按 SQL 中的关系名解析当前 operation catalog，并保持现有 DataFusion `DataFrame` 返回类型。关系不存在时直接报告 table-not-found，不调用任何 Provider 兜底，也不自动发现 Provider 内部或远端原始表。自动注册后没有被 SQL 引用的 `Table` 不会被 DataFusion 扫描，但创建它的 `Provider.query()` 已经执行。

由于 query 结果在 `ctx.sql()` 规划前已经具有可读的本地 backing、Schema 和 catalog 名，不需要惰性远端 TableProvider，也不存在 `ctx.sql()` 的双返回类型。

### D28：Runtime 不在 `Provider.query()` 返回后执行来源 SQL

一次 Provider 查询的概念顺序固定为：

```text
KAT 校验或生成 name，并在 operation 内保留名称与候选 Output 路径
  -> Runtime 分配 query-local scratch
  -> 进入 Source executor context，执行来源 SQL / 解析 / 私有本地查询
  -> Source executor 交付 Arrow batches 或 ParquetSource
  -> Runtime sink 写完私有 partial
  -> 正常退出 executor context，关闭 query-local 资源
  -> Runtime 把 partial finalize 为 outputs/<name>.parquet
  -> KAT 自动注册 name
  -> Provider.query() 返回本地 Table
```

若 Source executor 已有满足消费期不可变要求的 `ParquetSource`，Runtime 按 D37 转移 scratch 内产物，或把 scratch 外来源 clone/复制成独立 backing，避免解码后重写。无论采用哪条路径，`query()` 返回后 Runtime、`ctx.sql()` 和 Output publisher 都不得再次执行该来源 SQL。

Datasource factory 通过显式 `ctx.provider(executor)` 创建 KAT Provider facade；facade 再调用 Source executor 并连接 Runtime sink。D33 所定义的公开 Authoring 协议以 context-managed `pyarrow.RecordBatchReader | ParquetSource` 流式交付一个具有明确 Schema 的结果，不暴露 Runtime landing path，也不要求结果先收集成内存 `pyarrow.Table`。

### D29：`query(name=...)` 统一结果文件名、Table 名与 SQL 表名

`Provider.query(sql, params=..., name=None)` 的 `name` 同时表示：

- 候选 Run 中 `outputs/<name>.parquet` backing path 的名称；
- 返回值的 `Table.name`；
- 当前 operation DataFusion catalog 中的关系名。

显式名称必须同时满足 KAT 可移植文件名和未引用 DataFusion 表标识符规则。未提供 `name` 时，KAT 使用 `q_<sha256>`，其中 hash 只基于来源 SQL 原始字符串的 UTF-8 字节，不解析或规范化 SQL，也不加入 Provider、配置或 `params`。因此相同 SQL 即使来自不同 Provider 或使用不同参数，也会产生相同缺省名。

名称在来源 I/O 前保留。同一 operation 内一旦重名就立即失败，不执行第二次来源查询，不覆盖文件，也不隐式复用第一次结果；调用方需要两份结果时必须显式指定不同 `name`。查询失败不会返回 `Table`，Context 立即变为不可发布；Runtime 尽力删除 partial 文件，整个候选 Run 随 Workflow 失败而不发布，因此不定义名称释放或同名重试语义。

`outputs/<name>.parquet` 只是候选 Output backing path。只有 Workflow 返回值明确发布的 Table 或 DataFrame 才进入成功 Run Manifest；仅用于融合的查询结果在所有 DataFrame Output 完成后尽力清理。若 `Table` 被直接返回，它的 `Table.name` 同时成为 Run Output 名。

### D30：直接返回 Table 时，`Table.name` 就是 Run Output 名

一张 `Table` 只有一个名字，不在 Workflow 返回边界另设 alias：

```python
table = pg.query("SELECT ...", name="telemetry")
return table                         # Run Output: telemetry
return {"telemetry": table}          # 等价的具名返回
```

若返回 Mapping，Table 项的 key 必须与 `Table.name` 完全一致；`return {"result": table}` 在 `table.name == "telemetry"` 时失败。不能把同一张 Table 用两个 Mapping key 发布成两个 Output。缺省命名的单张 Table 直接返回时，`q_<sha256>` 就是它的 Run Output 名。

DataFrame 保持现有规则：单个 DataFrame 的 Output 名为 `main`，Mapping 中的 DataFrame 由 key 命名。一个 Mapping 可以同时包含 Table 与 DataFrame，但每个 Table 项仍必须满足 key 等于 `Table.name`。

### D31：DataFrame Output 名不得与 Provider Table 名重名

Provider query 在 Workflow 执行期间已经占用 `outputs/<Table.name>.parquet`，而 DataFrame materializer 也按 Output 名写入同一路径。若 DataFrame Output 名与任意 Provider `Table.name` 相同，DataFrame 可能一边读取融合输入、一边覆盖同一 backing，因此 Runtime 必须在执行任何惰性 DataFrame 前拒绝整个返回值。

```python
pg.query("SELECT ...", name="main")
return ctx.sql("SELECT ... FROM main")  # 拒绝：DataFrame Output 也叫 main
```

直接返回该 `Table` 不构成冲突，因为发布的就是它已有的候选 backing。合法的融合写法为来源 Table 和最终 DataFrame 使用不同名字：

```python
pg.query("SELECT ...", name="telemetry")
return ctx.sql("SELECT ... FROM telemetry")  # DataFrame Output: main
```

该检查覆盖单个 DataFrame 的隐式 `main` 和 Mapping 中的所有 DataFrame key，并与既有“先验证完整返回形状，再执行任意惰性 Output”顺序一致。不自动改名、不覆盖，也不通过临时替换绕过冲突。

### D32：Datasource factory 显式接收 `ctx` 并返回 KAT Provider facade

自动命名、落盘和注册需要当前 operation 的 Output 目录、DataFusion Session 与 execution lease。KAT 不通过 `ContextVar`、模块全局变量或其他隐式“当前 Runtime”把这些能力暴露给任意 Python 对象；Workflow 必须把 `ctx` 显式传给 Datasource factory：

```python
pg = postgresql.provider(
    ctx,
    profile=profile,
    database=database,
)

table = pg.query("SELECT ...", name="telemetry")
```

Datasource factory 通过 `ctx.provider(executor)` 组合两个角色：

```text
Datasource-owned source executor
  + 当前 Workflow ctx
  -> ctx.provider(executor)
  -> operation-bound KAT Provider facade
```

Source executor 拥有来源连接、方言、解析、私有 catalog 和 Arrow/ParquetSource 生成；KAT Provider facade 拥有公共 `query(sql, params=..., name=...)`、名称保留、Runtime sink、自动注册、Table 构造和 execution lease 检查。Datasource 作者不编写 Parquet 路径、Output 发布或 DataFusion 注册代码。

`ctx` 不是 Provider 配置或 Workflow input，不写入 Run Manifest。Provider facade 只能在创建它的 operation 中使用，不能跨 Workflow、Run 或测试执行保存和复用。PACK 测试通过显式 Context fixture 创建 Provider，不依赖隐式全局状态。

### D33：Source executor 只向 Runtime 交付 Arrow stream 或稳定 Parquet

Source executor 与 Runtime sink 使用一个公开给 PACK 作者实现、只由 KAT Python Runtime 调用的最小端口：

```python
class SourceExecutor(Protocol):
    def execute(
        self,
        sql: str,
        params: object | None,
        *,
        scratch: Path,
    ) -> ContextManager[
        pyarrow.RecordBatchReader | ParquetSource
    ]: ...

    def close(self) -> None: ...
```

`execute()` 不接收 Query result name、Runtime landing path 或融合 DataFusion Session。远程数据库、文件 parser 和其他流式来源返回标准 `pyarrow.RecordBatchReader`；reader 在读取 batch 前提供 Schema，也必须能表达具有明确 Schema 的零行结果。已有满足消费期间不可变要求的单表 Parquet 可以返回 `ParquetSource`。executor 不直接返回整表 `pyarrow.Table`、DataFusion `DataFrame`、远端 cursor 或仍待 Runtime 理解的来源查询计划。

调用 `execute()` 本身只创建并返回 ContextManager，不取得连接、cursor、reader、parser process 或其他 query-local 资源；这些资源必须在 ContextManager 的 `__enter__` 内取得。若 `__enter__` 在完成前失败，它必须在传播错误前自行关闭已经取得的部分资源；一旦成功进入，facade 保证退出 context。Source executor 的 context 拥有它交付的 reader 及全部来源资源，Runtime sink 只在 context 内消费，不接管远端连接或 driver 对象的长期所有权。

Provider facade 先保留名称并分配 query-local scratch，再进入 `execute()` 的 context manager，在 context 内把结果完整交给 Runtime sink。无论来源执行、流读取或 Parquet 写入是否成功，facade 都退出 context，使 executor 有机会关闭或取消本次查询的 cursor、reader、parser 任务和临时状态。只有 sink 完成且 context 正常退出后，facade 才自动注册本地结果并返回 `Table`。

本地 DataFusion executor 在自己的 Python Session 中创建 DataFrame 后，以 `pyarrow.RecordBatchReader` 逐 batch 交付，不调用 `collect()`，也不把 DataFusion 引入 Rust parser binary。当前锁定的 DataFusion 54 / PyArrow 24 对 Parquet Join 直接使用 DataFrame Arrow C stream 时会阻塞首批读取，因此本地实现使用 DataFusion `execute_stream()` 的惰性 batch iterator 适配 `RecordBatchReader`；这只是 PACK 内的兼容实现，不改变公开 executor 端口，也不先收集整表。远程驱动可以直接提供或适配同一种 reader。本方案不再自建跨语言表协议。

query-local 资源在每次 `query()` 返回前关闭。连接池、客户端或私有 DataFusion Session 等 operation-scoped 资源可以保留给同一 Provider facade 的后续查询；facade 把 executor 绑定到当前 operation，并在 operation 结束时调用 `close()`。没有可复用资源的 executor 可以提供空实现。executor 的具体资源实现属于 Datasource，关闭时机和异常路径由 KAT 生命周期编排保证。

### D34：成功 Output 不因未发布中间文件清理失败而改判失败

Runtime 必须保留所有 Provider `Table` backing，直到 Workflow 返回值已经完成校验，所有惰性 DataFrame Output 已成功物化。随后，直接返回的 `Table` backing 和 DataFrame Output 继续作为 Manifest 候选；仅用于融合或未被使用的 Provider backing、query scratch 与其他 operation-local 中间产物由 Runtime 尽力删除。

如果所有声明的 Run Output 已经成功形成，但某个未发布 Provider Parquet 或其他中间产物删除失败，Runtime 只在 Operation log 记录 cleanup warning，仍允许 CLI 发布 Run Manifest。残留文件不加入 Manifest，也不能通过 `kat query --run` 发现；它只是 `KAT_DATA_HOME` 下可回收的缓存垃圾，不成为系统状态。把 Run 改判失败既不能消除该文件，也会丢弃已经正确完成的 Output。

该规则只适用于成功 Output 之后的不可见垃圾清理，不放宽来源查询、Runtime sink、Output 物化或 Manifest 发布本身的成功条件。Datasource 自己创建的跨 Workflow 长期 materialization 不属于 Runtime 的中间产物清理范围。

### D35：`Provider.query()` 失败会使当前 Context 不可发布

一次 Provider 查询只在全部步骤成功后形成可观察的 `Table`：

```text
保留 operation-local name
  -> 写入 outputs/.<name>.<opaque>.partial
  -> 完整消费 Source execution result 并关闭 sink
  -> 正常退出 executor context
  -> 形成 outputs/<name>.parquet
  -> 注册 operation-local relation
  -> 返回 Table
```

最终 backing 形成前，Runtime 只写同一候选 Run 内的私有 partial，不让 DataFusion catalog 或 Workflow 返回值看到半张表。来源执行、batch 读取、sink、context 退出、backing 形成或 catalog 注册任一步失败，都不返回 `Table`，并把当前 Workflow Context 标记为不可发布。Runtime 尽力删除 partial、最终候选 backing 和 query scratch；清理失败只作为附加日志，不覆盖最初错误。没有 Manifest 的候选目录始终不是 Run。

PACK 即使捕获 `query()` 抛出的异常，也不能恢复该 Context；后续 Provider 查询、`ctx.sql()` 或最终 Output materialization 必须拒绝执行。名称保持已消耗，不定义释放、同名重试、catalog 回滚后继续运行或切换 Provider 的平台语义。这样失败路径不需要一套与成功 catalog 平行的事务状态机。

Source executor 可以在单次 `execute()` 内实现来源特定的重连、重试、节点切换或缓存回退，但这些步骤必须共同表现为一次 public `query()`：成功时只交付一个完整结果，最终失败时仍毒化当前 Context。具有外部副作用的恢复动作仍不属于只读 `query()`。

### D36：`ctx.provider(executor)` 是唯一 Provider 构造入口

KAT 在 PACK Authoring API 中公开 `SourceExecutor`、`ParquetSource`、`Provider` 和 `Table` 类型，并在当前 Workflow `Context` 上提供唯一的 facade 构造能力：

```python
def provider(self, executor: SourceExecutor) -> Provider: ...
```

Datasource factory 是普通 Python 函数。它创建来源特定 executor，再显式交给当前 `ctx`：

```python
def provider(
    ctx: kat.Context,
    *,
    profile: str,
    database: str,
) -> kat.Provider:
    executor = PostgreSQLExecutor(
        profile=profile,
        database=database,
    )
    return ctx.provider(executor)
```

PACK 作者按结构实现 `SourceExecutor.execute()` 与 `close()`，不需要继承 KAT 基类。`Provider` 的公开构造器不形成 Authoring Interface，也不能被 PACK 继承或替换；`ctx.provider()` 检查 execution lease，把 executor 纳入当前 operation 生命周期，并返回 KAT 固定实现的 facade。其公共查询面仍只有 `query(sql, params=..., name=...) -> Table`。

`ctx.provider()` 不注册来源表，也不等同于已删除的 `ctx.register()`。只有返回的 facade 成功完成一次 `query()` 后，KAT 才自动注册该具名本地结果。KAT 不增加 Provider registry、decorator、entry point、模块发现或隐式当前 Context。

来源特定的长期物化、写入、维护或诊断能力不加入统一 Provider facade。Datasource 可以把它们定义为普通模块函数或专用 Workflow；只有需要产生统一查询结果时，才通过 `ctx.provider(executor)` 进入 KAT 生命周期。

### D37：ParquetSource 必须转成由 Runtime 独占的不可变 backing

`ParquetSource` 只把一个已有单表 Parquet 文件或分片目录交给 Runtime，不把来源路径直接暴露为 `Table` backing。Runtime 规范化并校验路径后，根据它与本次 query scratch 的关系采用两种固定策略：

- 位于 Runtime 分配的 query scratch 内：在 `execute()` context 退出前 move/rename 到候选 partial，转移所有权；executor 交付后不得继续依赖该路径。
- 位于 query scratch 外：视为借用输入，在 `execute()` context 退出前优先用文件系统支持的 copy-on-write clone 形成内容独立的候选；不支持时按字节复制。原始来源保持不变。

Runtime 不使用 hard link 或 symbolic link。二者仍共享或跟随来源内容，无法保证 `execute()` context 结束后 `Table` 继续不可变；本方案也不为它们增加跨 query 的不可变 lease、引用计数或来源锁。copy-on-write clone 必须具有写时分离语义，普通链接不能以 clone 名义进入该分支。

单文件和单表分片目录遵守同一所有权规则。Runtime 在形成最终名称和注册关系前验证候选 Parquet 可读且 Schema 明确；move、clone、复制或验证失败都按 D35 使当前 Context 不可发布。该路径避免 Arrow 解码与 Parquet 重编码，但明确不承诺所有平台零拷贝。

### D38：`outputs/<name>.parquet` 可以是文件或单表 Parquet dataset 目录

`outputs/<name>.parquet` 是 Runtime 私有的 canonical backing path，不承诺其文件系统节点类型。它可以是一个普通 Parquet 文件，也可以是表示同一逻辑表的 Parquet dataset 目录。`Table`、Run Manifest、Output ID、`ctx.sql()` 与 `kat query --run` 都只按 Output name 定位该路径，不新增 physical kind 字段，也不向 PACK 或用户暴露二者差异。

作为 backing 的目录必须至少包含一个可读 Parquet part；零行结果仍要包含一个携带 Schema 的零行 part。所有 parts 必须能由当前锁定的 PyArrow/DataFusion 作为同一 Schema 的一张表读取。Runtime 不从目录名或文件名推导业务表、分区列或多表 catalog；Source executor 必须在交付前把范围收敛为一张逻辑表，Runtime 只验证并采用整个 dataset。

Runtime 计算目录 backing 的 Output Schema 与总 `row_count`，并在 query、本地融合和后续 Run Output Query 中注册同一个 canonical path。现有 DataFrame Output 首版继续物化为单个 Parquet 文件；只有 `ParquetSource` 可以保留已有单表分片形态。物理形态不影响 Manifest 发布、未发布 backing 清理或 D37 的所有权规则。

### D39：多表根目录到来源表名的映射完全属于 Source executor

KAT 不定义多表根目录的目录布局、表名发现或路径到 relation 的映射。Source executor 可以从 File Parser 的 JSON response 读取显式表索引、接收普通 Python Mapping、自行扫描某种约定布局、读取来源 catalog，或使用完全不同的实现建立私有 catalog。Workflow 的 Source query 使用该 executor 暴露的来源表名；这些名字不自动进入融合 Session。

例如 Hitrace Parser 可以返回：

```python
{
    "sched_switch": "tables/sched_switch.parquet",
    "thread": "tables/thread.parquet",
}
```

Hitrace executor 在自己的 DataFusion Session 中注册这份映射并执行来源 SQL。其他 Datasource 不需要使用相同 JSON、`tables/` 子目录或 `<table>.parquet` 命名。KAT 不自动扫描根目录、不枚举其中的表，也不解释 Hive partition、文件 stem、子目录名或 parser metadata。

因此 `some_datasource.provider(ctx, root=...)` 只是该 Datasource factory 自己定义的便利 API，不是 KAT 的通用 Parquet Provider 或目录合同。KAT 可以提供创建私有 DataFusion catalog 的普通辅助函数，但其输入映射由 Datasource 给出，不能演变为平台级 Dataset discovery。

整个多表根目录不能作为一次 `ParquetSource` 交给 Runtime。Source executor 必须先通过来源 SQL 把范围收敛为一张逻辑表，再交付 `RecordBatchReader`，或只把该单表对应的文件/分片目录交付为 `ParquetSource`。Runtime 永远不从 Source execution result 中发现第二张表。

### D40：`ctx.datasource_root` 提供唯一的 PACK 级长期存储根

Workflow Context 新增只读路径能力：

```python
ctx.datasource_root: pathlib.Path
```

生产执行中，它返回当前 CLI 已选中 PACK 的 canonical 路径：

```text
KAT_DATA_HOME/datasources/<pack-name>/
```

KAT 在首次访问时创建并校验这个 PACK-scoped 根，但不暴露整个 `KAT_DATA_HOME`，避免把 `runs/`、`logs/`、`packs/` 等平台物理布局变成 PACK Authoring Interface。PACK 改名会自然得到新的目录；KAT 不迁移旧目录。

Datasource 自己选择该根下的子目录、文件、缓存键、格式、版本、完整性、覆盖、重建和清理规则。`ctx.datasource_root` 不提供 list、publish、delete、lock、Manifest、Artifact ID 或 registry。返回 `Path` 本身不是持久身份；后续 Workflow 仍只通过普通 input 传递由 Datasource 解释的 artifact key 或其他选择值。

Datasource factory 或专用物化函数可以把该根下的路径作为普通配置传给 Source executor；executor 不接收 Context，也看不到其他平台目录。该能力只防止正常作者误用平台根，不把受信任 PACK Python 伪装成文件系统沙箱。

`kat test` 把 `ctx.datasource_root` 映射到当前 pytest test 的 `tmp_path/datasources/<pack-name>/`。同一测试中的多次 `kat_run` 共享该测试级目录，以便验证先物化后复用；不同测试隔离，测试结束后不写入生产 Data Home。Context lease 结束后不能再次通过旧 Context 获取该路径，但已经生成的生产 materialization 按 Datasource 规则跨 Run 存在。

### D41：首个交付里程碑包含 Docker PostgreSQL 与本地 Parquet 的端到端融合

首个交付里程碑不能只用 fake executor 证明 Runtime 抽象。它由 D50 的两个可独立 review 切片共同完成，并最终交付：

- KAT Authoring API：`SourceExecutor`、`ParquetSource`、`Provider`、`Table`、`ctx.provider()`；
- Runtime 主干：eager sink、partial、自动注册、Context poison、Table Output、清理与 operation 生命周期；
- 一个接收显式 `Mapping[str, Path]`、使用私有 DataFusion Session 的本地 Parquet executor；
- PostgreSQL Datasource factory 与 PostgreSQL Source executor；
- 测试环境提供的 Docker PostgreSQL 服务参与真实端到端测试。

“PostgreSQL Provider”在该模型中不是 PACK 可继承或自行实现的 facade 子类。Datasource 模块创建 `PostgreSQLExecutor`，再调用 `ctx.provider(executor)` 返回统一的 KAT `Provider`：

```python
def provider(
    ctx: kat.Context,
    *,
    profile: str,
    database: str,
) -> kat.Provider:
    return ctx.provider(
        PostgreSQLExecutor(
            profile=profile,
            database=database,
        )
    )
```

该 PostgreSQL Datasource 按普通 PACK Python 代码和公开 Authoring API 实现，用于端到端证明远端来源合同；它不被提升为 KAT Runtime 内建 Provider。仓库中现有 Bundled PACK 是否消费 PostgreSQL 不参与本方案取舍，也不属于该测试的验收条件。

端到端测试以一个可由测试进程访问的 Docker PostgreSQL 服务为前提，在 `telemetry` Database 准备可由同一来源 SQL Join/Filter 的远端表，在 `control` Database 准备进程维表，并提供一份显式映射的本地 Parquet 表。测试 Workflow 必须先通过两个 PostgreSQL Provider 分别本地化 `telemetry` 与 `processes`，再通过本地 executor 产生 `switches`，最后用 `ctx.sql()` 按三个 Query result name 联合结果并验证最终 Run Output。这样同时证明“同库 SQL 由来源执行”“一个 Workflow 依次查询多个 Database”和“跨来源 SQL 只读取本地 backing”。

第二个 PR 还要覆盖 PostgreSQL 参数安全绑定、零行 Schema、远端错误使 Context poison、`RecordBatchReader` 流式落盘，以及每次 query 返回前对应远端连接已经按合同关闭。测试凭据只属于测试 fixture，不进入生产示例、Run Manifest 或日志。

首个里程碑仍不迁移 Hitrace parser、不删除旧 Dataset/`required_tables`/`kat import`，也不引入 Binding、远端 TableProvider 或自动 pushdown。Docker 只提供测试服务，其 image 选择、创建、健康检查与销毁属于测试环境，不是 Datasource、Provider 或 Runtime 合同；本 SDD 不设计容器生命周期。

### D42：首个 PostgreSQL executor 只采用 ADBC，并以真实 Arrow 流为硬门槛

首个 PostgreSQL executor 固定使用 `adbc-driver-postgresql`，不同时支持第二套驱动。选择它的原因是来源结果可以直接以 Arrow stream 交付给 Runtime sink，从而避免 KAT 自行维护 PostgreSQL 类型到 Arrow 类型的通用转换、逐行装批与两套资源生命周期。

Docker 端到端测试必须证明当前锁定版本可以完成以下合同：

- 使用驱动原生参数绑定执行返回结果集的参数化 `SELECT`；
- 在不先收集为 `pyarrow.Table` 的前提下取得可消费的 `RecordBatchReader`；
- 零行结果仍具有可写入 Parquet 并可注册的明确 Schema；
- setup statement、query statement、reader、transaction 与 connection 在每次 query 的成功和错误路径上都能关闭。

这些不是可跳过的兼容性测试，而是 PostgreSQL 切片成立的实现门槛。任一合同在当前锁定版本或目标平台上不成立，就阻塞该切片并重新做驱动决策；首版不静默回退到 `psycopg`，也不在 KAT 内手写 PostgreSQL 到 Arrow 的类型转换。测试只连接外部提供的 Docker PostgreSQL，不因此增加容器管理能力。

当前 PACK 没有独立依赖声明或安装机制，Bundled Python Host 也不读取用户环境中的 site-packages。因此 `adbc-driver-postgresql` 属于 Workflow Host 的基础生产依赖，而不是 test dependency、optional extra 或 PACK dependency；实现切片必须在 `kat/platform/workflow/pyproject.toml` 精确锁定验证版本，并重新生成 Linux 与 Windows 两份平台 requirements lock。平台 binary-wheel、native library 闭包和 Payload 基线检查与真实 PostgreSQL 测试共同构成交付门槛。

### D43：PostgreSQL 只读由最小权限身份、服务端事务与单 command 路径共同保证

首个 `PostgreSQLExecutor` 不引入或自行实现 PostgreSQL SQL parser。SQL 仍由受信任 PACK 选择，生产 Datasource profile 必须解析为只读 PostgreSQL 身份；密码、token、含凭据 DSN 及凭据文件内容不进入 Workflow input、Run Manifest 或 Operation log。

每次 Source query 禁用 autocommit，并在执行用户 SQL 前通过独立的内部 statement 设置当前事务为 `READ ONLY`。当前 ADBC PostgreSQL driver 尚不能仅凭 ADBC 标准的 connection read-only option 建立这一保证，因此实现与测试必须验证实际服务端 `transaction_read_only` 为 `on`，不能只验证客户端配置调用成功。内部事务语句不得与用户 SQL 拼接。

用户 SQL 始终走请求 Arrow result stream 的 prepare/bind/execute 路径；即使没有 `params`，也不切换到允许多个 command 的无结果执行路径。该路径必须由锁定驱动版本与真实 PostgreSQL 测试证明只接受一个 command。Runtime 完整消费 stream 后，无论查询成功还是失败都回滚只读事务；零列 command result 不能伪装成 `Table`。

测试分开证明三条防线：

- 在 executor 级负向测试中用具有写权限的 fixture 身份证明 `READ ONLY` 事务确实拒绝 `INSERT ... RETURNING`、DDL、`COPY FROM` 和 data-modifying CTE，且来源事实未变化；该身份不作为公开 Datasource E2E 的生产形态 profile；
- 用生产形态的只读 fixture 身份证明其自身没有对象写权限或高权限角色，同时正常 Join、Filter 与参数化 `SELECT` 可执行；
- 证明 `SELECT 1; SELECT 2` 被拒绝，而字符串、注释和 dollar-quoted 内容中的分号不被错误拆分。

这套合同承诺“服务端阻止 PostgreSQL 持久写入，并且只接受一个具有明确 Schema 的表结果”，不把 `Provider.query()` 宣称为不可信 SQL 沙箱。只读事务不能判断任意函数是否具有数据库外部副作用；生产 profile 仍必须使用最小权限身份和受控 relation/function。KAT facade 不对所有来源建立统一的 SQL 安全检查，其他 Source executor 继续对自己的查询边界负责。

### D44：首个 PostgreSQL executor 每次 query 使用独立连接

首版不在 `PostgreSQLExecutor` 中维护 operation-scoped connection 或 connection pool。每次 `execute()` 打开一个新 ADBC connection，建立 D43 的只读事务，在连接仍存活时把 Arrow stream 交给 Runtime 完整消费，随后无论成功失败都回滚并关闭 statement、reader、transaction 与 connection。`Provider.query()` 返回本地 `Table` 后不再存在对应远端 session。

因此首版 `PostgreSQLExecutor.close()` 不承担正常连接回收，只是满足通用 Source executor 生命周期合同的幂等兜底；正常情况下它没有尚存的远端资源可关闭。setup statement、query statement、reader、transaction 与 connection 的所有权都属于单次 `execute()` context。

同一 Provider 在一个 Workflow 中连续查询时只复用 profile、Database 等普通 executor 配置，不复用远端连接。这会承担每次查询的建连成本，但能在首个切片中隔离 failed transaction、临时对象、session 设置、advisory lock 等 query-local 或 session-level 状态，并让资源关闭可直接验证。若真实性能证据表明建连成为瓶颈，Datasource 以后可以在不改变 `Provider`、`SourceExecutor` 或 Workflow API 的前提下，内部增加 pool；该优化不进入首个切片。

### D45：PostgreSQL executor 原样交付 ADBC Arrow 类型

首个 PostgreSQL executor 不在 ADBC 之上维护第二套 PostgreSQL 到 Arrow 的类型映射、归一化或逐值转换。它原样交付 ADBC 返回的 Arrow Schema 与 RecordBatch；Runtime sink 只负责把标准 Arrow stream 写成候选 Parquet，并把形成的本地 Table 注册给 DataFusion。

如果 Workflow 对来源类型有特定语义要求，应在 PostgreSQL Source query 中使用显式 `CAST`，由数据库执行转换。若某种 PostgreSQL 类型不能被锁定版 ADBC 稳定流式返回、不能写入当前 Parquet，或不能由当前 DataFusion 注册读取，则本次 `query()` 失败并按既有规则 poison Context；KAT 不猜测替代类型，也不在 facade 中静默转成字符串或二进制。

首个端到端测试只固定实际融合链路需要的一组代表类型及其精确 Arrow Schema，包括整数、浮点、布尔、文本、时间和空值，并覆盖这些类型的参数绑定与 Parquet/DataFusion 往返。通过该矩阵只承诺锁定版本下这些已验证类型，不宣称支持 PostgreSQL 全部内建、扩展或用户定义类型。新增类型支持属于 PostgreSQL Datasource 的增量验证，不改变通用 Provider 合同。

### D46：operation-level executor close 是尽力清理，不是查询提交

Workflow 函数返回或抛错后，Runtime 都调用当前 operation 已创建的全部 Source executor 的 `close()`，并在某个 close 失败后继续关闭其余 executor。`close()` 只释放仍存的 operation-scoped pool、私有 Session、cache handle 或其他资源，不得提交来源查询、补完 `Table`、修改已形成的 backing 或承担 Run Output 正确性。

如果 Workflow 与此前所有 query 均成功，operation-level `close()` 失败只记录 cleanup warning，Runtime 仍可继续验证、物化并发布返回的 Output；使 Run 失败既不能完成资源关闭，也不能提高已经本地化结果的正确性。如果此前已有 query、Workflow 或 Output 错误，则保留第一个业务错误作为主诊断，所有 close 错误只作为附加 warning，不覆盖它。

这不放宽单次 query 的成功条件。`execute()` 的 `__enter__` 必须自行清理进入失败前取得的部分资源；成功进入后，context 的正常退出仍发生在 partial finalize、自动注册和 `Table` 返回之前。`__exit__` 失败会使该 query 失败并 poison Context。只有 Workflow 已经结束后调用的 executor `close()` 使用本节的 best-effort 规则。

### D47：PostgreSQL profile 直接使用 libpq service

首个 PostgreSQL Datasource 的 `provider(ctx, *, profile, database)` 不建立 KAT profile registry。`profile` 是 libpq connection service file 中的 service 名；`database` 是本次 Provider 明确选择的 Database，并覆盖 service 中可能存在的默认 Database。Workflow 可以用同一 `profile` 和不同 `database` 创建多个 Provider，从而依次查询同一服务上的多个 Database，再把各自已本地化结果交给 `ctx.sql()`。

service file 管理 host、port、user、TLS 及其他非敏感连接策略，password file 或外部凭据机制管理秘密。Workflow input、Run Manifest 与日志只记录 `profile` 和 `database`，不接受或输出 DSN、用户名、密码、token、service/password file 内容、原始环境 Mapping 或任意 connection kwargs。Datasource 在实际 Workflow 执行期解析连接配置；PACK import、inspection 与 factory 定义阶段不读取凭据或建立连接。

生产环境沿用 libpq 的默认 service/password file 查找规则及其标准环境覆盖。端到端测试通过 `PGSERVICEFILE` 与 password-file 配置指向测试专用文件，并把只读 service 名和 Database 作为 Workflow inputs；Docker 地址与测试秘密不进入生产示例。Datasource 负责把 service 与 Database 安全组合为 ADBC 接受的 libpq connection URI/参数，并且错误诊断不得回显完整连接字符串。

### D48：PostgreSQL 案例以可运行 External example PACK 提交

首个 PostgreSQL 与本地 Parquet Datasource 不只存在于测试函数或 SDD 代码片段中。仓库在 `examples/packs/postgresql-parquet-fusion/` 新增一个完整可运行的 External example PACK，包含 `pack.toml`、`helpers/datasources/` 下的 PostgreSQL 与 Parquet 实现、一个跨源 Workflow、PACK tests 和面向作者的 README。它通过普通 `--pack-dir` 参与 discovery、inspection、test 与 run，不进入默认 Bundled PACK 集合。

example PACK 中的 Datasource 是普通生产形态 PACK Python：PostgreSQL factory 创建 `PostgreSQLExecutor`，本地 factory 创建私有 DataFusion executor，两者都只通过公开 `ctx.provider(executor)` 取得 KAT Provider facade。它们不是 KAT Runtime 内建 Provider，也不进入 Pack Authoring API；其他 PACK 可以阅读或复制该模式，但本方案不因此建立跨 PACK import、共享 helper registry 或 PACK dependency。

Docker PostgreSQL 端到端测试直接运行这一个 example PACK 的公开 Workflow，并复用 `helpers/datasources/` 中同一份代码，不另写一个行为不同的 test-only PostgreSQL executor。测试专用 service/password file、远端初始化数据与本地 Parquet fixture 位于测试环境，案例源码和 README 不包含凭据或固定 Docker 地址。这样一次验证同时证明外部 PACK 加载、Datasource 自定义、远端来源查询、Runtime 本地化与跨源融合。

### D49：主案例依次查询两个 PostgreSQL Database 与本地 Parquet

example PACK 使用一个 libpq service、两个 PostgreSQL Database 和一个本地 Parquet catalog。`telemetry` Database 至少包含 `observation` 与以 `thread_id` 唯一的 `thread_registry`，其 Join 和半开时间窗 `[start_ns, end_ns)` 过滤在同一条 PostgreSQL Source query 中完成；`control` Database 提供以 `process_id` 唯一的小型 `process_registry` 维表；本地 catalog 显式映射包含 `cpu`、`next_thread_id` 与 `timestamp` 的 `sched_switch` Parquet，测试 fixture 保证 `(cpu, timestamp)` 唯一。Workflow 分别得到三个自动注册的本地 `Table`，最后由 `ctx.sql()` 融合：

```python
from pathlib import Path

import kat

from kat.pack.helpers.datasources import parquet, postgresql


@kat.workflow(
    name="fuse-observations",
    title="Fuse PostgreSQL observations with local scheduling",
    required_tables=[],
    parameters={
        "profile": "libpq service name.",
        "telemetry_database": "Database containing observations.",
        "control_database": "Database containing process metadata.",
        "trace_root": "Directory containing sched_switch.parquet.",
        "start_ns": "Inclusive observation window start.",
        "end_ns": "Exclusive observation window end.",
    },
)
def fuse_observations(
    ctx: kat.Context,
    profile: str,
    telemetry_database: str,
    control_database: str,
    trace_root: str,
    start_ns: int,
    end_ns: int,
):
    if start_ns >= end_ns:
        raise ValueError("start_ns must be less than end_ns")

    telemetry = postgresql.provider(
        ctx,
        profile=profile,
        database=telemetry_database,
    ).query(
        """
        SELECT
            o.thread_id,
            r.process_id,
            o.observed_at,
            o.cpu_usage
        FROM observation AS o
        JOIN thread_registry AS r USING (thread_id)
        WHERE o.observed_at >= $1
          AND o.observed_at < $2
        """,
        params=(start_ns, end_ns),
        name="telemetry",
    )

    processes = postgresql.provider(
        ctx,
        profile=profile,
        database=control_database,
    ).query(
        """
        SELECT process_id, process_name
        FROM process_registry
        """,
        name="processes",
    )

    switches = parquet.provider(
        ctx,
        tables={
            "sched_switch": Path(trace_root) / "sched_switch.parquet",
        },
    ).query(
        """
        WITH intervals AS (
            SELECT
                cpu,
                next_thread_id,
                timestamp AS run_start,
                LEAD(timestamp) OVER (
                    PARTITION BY cpu
                    ORDER BY timestamp
                ) AS run_end
            FROM sched_switch
        )
        SELECT cpu, next_thread_id, run_start, run_end
        FROM intervals
        WHERE run_start < $end_ns
          AND run_end > $start_ns
        """,
        params={"start_ns": start_ns, "end_ns": end_ns},
        name="switches",
    )

    return ctx.sql(
        """
        SELECT
            t.thread_id,
            t.process_id,
            p.process_name,
            t.observed_at,
            s.cpu,
            s.run_start,
            s.run_end,
            t.cpu_usage
        FROM telemetry AS t
        JOIN processes AS p USING (process_id)
        JOIN switches AS s
          ON t.thread_id = s.next_thread_id
         AND t.observed_at >= s.run_start
         AND t.observed_at < s.run_end
        ORDER BY t.observed_at, t.thread_id
        """
    )
```

最终行粒度是一条位于 `[start_ns, end_ns)` 内、能够匹配唯一 thread registry、唯一 process 和一个已知 CPU 运行区间的 observation。测试 fixture 以约束或显式断言保证 `thread_registry.thread_id`、`process_registry.process_id` 与 `sched_switch(cpu, timestamp)` 各自唯一，并且同一线程的运行区间不重叠；缺少任一维表或落在未知区间的 observation 由 inner join 排除，不产生多对多放大，窗口顺序也保持确定。

执行顺序严格由 Python 调用顺序决定：先访问 `telemetry` Database 并关闭连接，再访问 `control` Database 并关闭连接，然后用私有 DataFusion 查询本地 Parquet，最后的 `ctx.sql()` 只读取 `telemetry`、`processes`、`switches` 三个已形成的 Parquet backing。最终 DataFrame 以现有单值规则发布为 `main`；三个来源 Table 仅作为融合输入，不进入 Manifest，并在 `main` 完成后尽力清理。

### D50：首个里程碑拆成 Runtime/local 与 PostgreSQL example 两个 PR

本 SDD 的首个交付里程碑分成两个可独立 review 和验证的 tracer bullet，不把 Runtime 主干、两个 executor、第三方驱动与真实服务测试塞入一个大 PR。

第一个 PR 交付通用 Runtime 与本地能力：

- Pack Authoring API 中的 `SourceExecutor`、`ParquetSource`、`Provider`、`Table` 与 `ctx.provider()`；
- 只读 `ctx.datasource_root` 及其生产 PACK-scoped、测试隔离路径注入；
- query-local workspace、eager Arrow/Parquet sink、private partial、finalize、自动注册、Context poison、Table Output、清理与 executor 生命周期；
- 接受显式 `Mapping[str, Path]` 的 PACK 层本地 Parquet/DataFusion executor；
- fake executor 合同测试，以及多个本地 Provider Table 经 `ctx.sql()` 融合的集成测试。

第一个 PR 本身已经形成可运行的本地 Datasource 垂直切片，不发布 PostgreSQL 未完成能力，也不加入 ADBC。它的验证证据必须覆盖成功流、零行 Schema、stream 中途失败、sink/finalize/register 失败、名称冲突、poison、直接 Table Output、融合中间表清理与 operation-level close warning。

第二个 PR 交付 PostgreSQL 证明：

- 精确锁定 ADBC 生产依赖并重生成 Linux/Windows 平台 requirements lock；
- D48 的 repository example PACK 及 README；
- D42-D49 的 PostgreSQL executor 与可运行案例合同；
- 测试环境提供的 Docker PostgreSQL 上两个 Database 与本地 Parquet 的 D49 端到端测试，以及参数、只读、单 command、类型和资源错误路径测试。

只有第二个 PR 的真实服务门槛全部通过后，里程碑才宣称 PostgreSQL Datasource 案例已经交付。旧 Dataset、`required_tables`、`kat import`、Hitrace parser 迁移与旧实现删除都不进入这两个 PR；它们在新主干有实际验证后作为后续独立切片处理。

### D51：迁移期允许旧 Dataset relation 与新 Provider Table 自然共存

在旧 Dataset、`required_tables` 与相关 Workflow 尚未迁移的过渡期，Runtime 继续按现有路径先把已解析 Dataset relations 注册到 Workflow DataFusion catalog。新 Provider query 与这些 relation 共用同一个 operation-local catalog；`ctx.sql()` 可以自然联合旧 relation 和新 Provider 已本地化 Table，不增加 Dataset-to-Provider adapter、Binding、别名层或两套融合 API。

Provider facade 在来源 I/O 前检查生成或显式指定的 Query result name 是否与当前完整 catalog 中的任意 relation 重名，包括旧 Dataset table、此前 Provider Table 及其他已注册 relation。重名立即失败并 poison 当前 Context，不执行来源查询、不覆盖、不 shadow，也不等到落盘后注册时才发现冲突。

这只是迁移期间复用现有 Session 的兼容行为，不是目标领域模型。D48 的 example PACK 完全使用新 Datasource 模型，不依赖旧 Dataset；在现有 decorator schema 删除前只声明 `required_tables=[]`，不把该空列表解释为来源合同。后续真实 PACK 迁移完成并删除旧模型后，Dataset relation 分支自然消失；Provider、Table、`ctx.sql()` 与 Run Output 合同不需要随之改变。

### D52：Linux 与 Windows 都必须通过真实 PostgreSQL 测试

PostgreSQL Datasource 的生产支持范围与当前 Workflow Host 的 Linux、Windows 目标一致，不能用 wheel 可安装或 Payload 可构建代替真实驱动验证。两个平台都必须以非 skip 测试连接一个可访问的真实 PostgreSQL 服务，执行 D42-D49 的参数化结果查询、Arrow stream、零行 Schema、只读事务、单 command、类型矩阵、错误关闭与完整 example PACK 融合流程。

测试基础设施可以用 Docker 或其他方式提供同一版本的 PostgreSQL，但 KAT 测试与 Datasource 合同只接收可访问的 libpq service，不创建、等待、销毁或复用容器。任一受支持平台未执行真实测试或测试不通过，就不能宣称该平台已经支持 PostgreSQL Datasource；实现不得以另一平台通过或 binary wheel 存在作为替代证据。

## 6. 交付验收矩阵

| 切片 | 必须验证 | 最小证据 |
|---|---|---|
| PR 1 / Authoring API | PACK 自定义结构化 executor，无需继承；Provider 只能由 `ctx.provider()` 创建 | inspection/type/API tests |
| PR 1 / Provider lease | Workflow 结束后的 Provider 在任何来源 I/O 前拒绝使用；正常与异常结束都把每个已登记 executor 恰好 close 一次 | spy executor lifecycle tests |
| PR 1 / Datasource root | 生产路径只暴露当前 PACK 的 `KAT_DATA_HOME/datasources/<pack-name>/`；同一 pytest test 内共享、不同 test 隔离；失效 lease 不能继续取根 | Context/path integration tests |
| PR 1 / Arrow sink | 多 batch、零行但有 Schema、大结果不先收集成 `pyarrow.Table` | fake `RecordBatchReader` integration tests |
| PR 1 / ParquetSource | 单文件、单表分片目录、scratch 内转移、scratch 外独立复制；拒绝 link 与多表根 | 文件系统 integration tests |
| PR 1 / query 原子性 | enter、stream、sink、context exit、finalize、register 任一步失败均无 Table、无 Manifest并 poison Context | 每个 failure point 的 fault-injection test |
| PR 1 / 名称 | 显式名、SQL hash 缺省名、全 catalog 重名在来源 I/O 前失败 | spy executor + catalog tests |
| PR 1 / Output | 单 Table、Table Mapping、DataFrame、混合 Mapping；拒绝 Table key 不一致及 DataFrame 名冲突 | Output materializer tests |
| PR 1 / 清理 | 融合输入保留到 DataFrame 完成；未发布 backing 与 scratch 尽力清理；任一 operation close 失败只告警、不阻断其余 executor close、也不覆盖主错误 | success/failure cleanup tests |
| PR 1 / 本地 executor | 显式多表 Mapping、私有 DataFusion Session、`$name` 参数、结果自动注册并参与融合 | PACK-level local fusion test |
| PR 1 / 迁移共存 | 旧 Dataset relation 与 Provider Table 可联合；重名不 shadow | legacy/new integration test |
| PR 2 / 依赖闭包 | 锁定 ADBC，Linux/Windows 只使用允许的 binary wheels/native libraries | 两个平台 lock 与 Payload build evidence |
| PR 2 / 平台矩阵 | Linux、Windows 均以非 skip 方式连接真实 PostgreSQL 并运行 executor 合同与 D49 example PACK | 两个平台实际测试报告 |
| PR 2 / profile 与秘密 | libpq service + Database override 可用；Manifest、Response、Operation log 与异常不包含凭据 | service-file E2E + redaction assertions |
| PR 2 / 参数与 command | `$1` 单组参数化 SELECT 返回 stream；多 command 失败；文本内部的分号不误判 | 真实 PostgreSQL driver tests |
| PR 2 / 只读 | 服务端 `transaction_read_only=on`；写权限测试角色仍无法 DML/DDL/COPY；生产角色最小权限 | 真实 PostgreSQL negative tests |
| PR 2 / 类型 | 锁定代表类型的精确 Arrow Schema、参数绑定及 Parquet/DataFusion 往返；未验证类型不扩张承诺 | schema matrix test |
| PR 2 / 生命周期 | 每次 query 使用不同 backend/connection；返回前已 rollback/close；各错误路径无残留 session | backend PID 与 fault-injection evidence |
| PR 2 / 完整案例 | 同一 service 的两个 Database 依次查询；同库 Join/Filter 远端执行；本地 Parquet 查询；关闭远端后 `ctx.sql()` 仍完成最终 `main` | Linux、Windows 上 D48 example PACK 的真实 PostgreSQL E2E |
| PR 2 / 作者体验 | README 中的 inspect、test、run 命令与结果可复现，不依赖默认 Bundled PACK | 实际命令输出证据 |

Docker 只负责在测试环境提供可访问的 PostgreSQL 服务。镜像选择、容器创建、等待、销毁和复用策略不属于本文接口或验收模型。

## 7. 明确不做

- 不实现透明跨源 SQL、Federation planner、远端 TableProvider 或自动 pushdown。
- 不建立 Datasource/Provider registry、Binding、平台 Dataset、统一 materialize 命令或统一来源 catalog。
- 不把任意 SQL 当成不可信代码沙箱；Workflow 与 PACK 仍是受信任代码。
- 不增加跨 PACK import、PACK dependency、共享 datasource helper registry 或现有 Bundled PACK 迁移。
- 首个里程碑不迁移 Hitrace、不独立暴露 Ftrace parser、不删除旧 Dataset/`required_tables`/`kat import`。
- 不在首版实现连接池、跨 Provider 并行、查询级缓存、自动重试或长期物化可靠性框架。

本文没有剩余的未确认设计分支。实现中若发现 ADBC 硬门槛不成立，或必须扩大上述公共面，应停止对应切片并回到 issue/SDD，而不是增加隐式 fallback。
