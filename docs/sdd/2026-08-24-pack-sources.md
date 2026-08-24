# PACK Sources 与 Dataset Binding 实现 SDD

## 要解决的问题

变更前，系统仍以封闭的 `kat import`、扁平 Dataset、Workflow `required_tables` 和匿名表集合为中心。它无法落实 ADR-0062 已确认的 Sources / Analysis / Workflows 能力模型，也不能让 PACK 在不修改平台枚举的前提下接入文件、数据库或现有查询设施。

本次实现交付以下完整纵向能力：

- PACK 通过 Source Entry 提供 DataFusion Source schema；
- Dataset 按 `(PACK identity, Source name)` 保存唯一 Binding；
- External Binding 在实际访问表时才调用 Source Entry；
- Materialized Source 通过本地 Parquet 提供相同的 catalog/schema 查询面；
- Workflow 不再声明静态表依赖；
- `kat bind`、`kat materialize`、`kat query --dataset` 与 `kat_run(sources=...)` 形成闭环；
- 一个普通日志 PACK 证明 Framer / Decoder、Arrow reader、Source staging、Analysis 与 Workflow 可以共同工作。

Hitrace 的官方 FFI Source 与顶层 `kat import`、Deprecated Trace Streamer 的退场由后续 [Hitrace Source SDD](2026-08-25-hitrace-source-and-import-removal.md) 完成。本切片不增加匿名 legacy namespace、表名猜测或自动迁移。

## 明确不做

本次不增加：

- PG、Excel、Flight SQL 的官方专用 Provider；
- 通用 Rust 与 Python Provider 桥；
- 多 Dataset overlay、merge 或 alias；
- 同一 PACK Source 的多实例 Binding；
- Source 专用查询、解绑或删除命令；
- 来源哈希、一致性、覆盖范围或业务完整性校验；
- Binding 版本、PACK 锁定、迁移或兼容分叉；
- 凭据加密、脱敏或安全擦除保证；
- Dataset 并发锁、CAS、事务、回滚或崩溃恢复；
- 通用 Framer、Parser 注册表或任意 generator 协议；
- reader helper 的动态谓词下推。

## 1. PACK Authoring API

### 1.1 `@kat.source`

公共签名固定为：

```python
def source(*, name: str) -> Callable[[F], F]: ...
```

Source Entry 必须是定义在模块顶层、非 lambda 的普通同步函数。它不能是 coroutine、generator 或 async generator；参数只能是 `POSITIONAL_OR_KEYWORD` 或 `KEYWORD_ONLY` 具名参数，不接受 positional-only、`*args`、`**kwargs`、`Context`、`SourceContext` 或完整 inputs mapping。

Source name 完整匹配 `^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$`，在 PACK 内唯一，并遵守可移植文件名规则：拒绝 Windows 设备保留名 `con`、`prn`、`aux`、`nul`、`com1`–`com9` 和 `lpt1`–`lpt9`，也拒绝 KAT 保留的 `dataset` 与 DataFusion 保留的 `information_schema`。Decorator 只声明该身份，不重复声明 tables、Schema、参数说明或版本。Inspection 不求值 return annotation；实际调用时，返回值必须是 DataFusion 可注册的 `Schema`、Python `SchemaProvider` 或官方 FFI SchemaProvider 值，其他值直接失败。

Source Entry 与 Workflow 分开记录注册。`sources/` 和 `workflows/` 使用同一套确定性入口规则：递归扫描普通 `.py`，禁止 `__init__.py`，每个入口文件恰好注册一个由当前模块定义的对应入口函数，并拒绝间接注册和 module/file 冲突。

### 1.2 `@kat.workflow`

公共签名删除 `required_tables`：

```python
def workflow(
    *,
    name: str,
    title: str,
    parameters: dict[str, str] | None = None,
) -> Callable[[F], F]: ...
```

不保留兼容参数或 alias。旧代码继续传入 `required_tables` 时，由 Python 函数签名直接拒绝。Workflow inspection 只投影 `name`、`title`、`description` 和 `parameters`；执行前不再建立 Table Grant，也不检查静态表集合。

## 2. Source Input Compiler

现有 Workflow Input Compiler 拆成私有通用内核与两个 profile：Workflow profile 保持现有标量、说明文字和默认值合同；Source profile 复用标量合同，不使用参数说明，并增加 Path。

Source profile 支持：

- `str`；
- signed int64 `int`；
- finite `float`；
- 有默认值的 `bool`；
- `kat.Duration`；
- `kat.WallClockTimestamp`；
- string `Literal`；
- 上述非 bool 单值类型的 `T | None`；
- `pathlib.Path` 与 `pathlib.Path | None`；
- `tuple[pathlib.Path, ...]`。

`tuple[Path, ...]` 不再套 Optional，空 tuple 已能表达“没有文件”。其他 tuple、list、dict、model 与自定义 parser 均拒绝。

Path 使用 `click.Path(path_type=Path, exists=False, resolve_path=False, allow_dash=True)` 的解析语义。解析后，绝对 Path 保持绝对；相对 Path 以 request 显式提供的 `argument_base` 为基准，通过纯词法绝对化得到传给 Source Entry 的 Path，不要求目标存在，也不解析符号链接。普通 `str` 永远不按路径解释；KAT 不展开环境变量、`~`、模板或 response file。

可重复 Path 参数复用同一个 option，例如 `--files a.log --files b.log`。没有默认值时至少出现一次；默认值为 `()` 时可以省略；Source Entry 最终仍收到 `tuple[Path, ...]`。

Source parameter inspection 的字段为：

```json
{
  "name": "files",
  "option": "--files",
  "type": "path",
  "required": false,
  "repeatable": true,
  "default": []
}
```

单个 Path 参数与可重复 Path 参数的 `type` 都是 `path`，只有后者输出 `repeatable: true`。Path 默认值投影为 string，可重复 Path 的默认值投影为 string array；int64、Duration 与 WallClockTimestamp 延续现有 string 投影。Source parameter 不含 `description`；bool 与 Literal 延续现有 `negative_option` 和 `choices`。

## 3. PACK Inspection 与 Source Guide

Python Runtime 的 inspection result 精确为：

```json
{
  "source_guide": "...或 null",
  "sources": [
    {"name": "raw_smaps", "parameters": []}
  ],
  "workflows": []
}
```

Sources 按 Source name 排序，Workflows 按 Workflow name 排序，parameters 保持函数签名顺序。

`SOURCES.md` 使用严格 UTF-8 读取并返回完整文本，不 trim，也不规范化换行。存在 Source Entry 时，文件缺失、不是普通文件、不可读或不是 UTF-8 都会失败；没有 Source Entry 且文件不存在时返回 `null`；没有 Source Entry 但文件已经存在时仍读取，读取失败仍报错。

各操作使用以下 profile：

- `kat inspect --pack`：扫描 Sources 与 Workflows，执行 Guide 门禁；
- `kat test`：扫描 Sources 与 Workflows，执行 Guide 门禁；
- `kat bind`、`kat materialize`：只扫描 Sources，执行 Guide 门禁，不因无关 Workflow 错误失败；
- `kat run`：扫描 Sources 与 Workflows，但不执行 Guide 门禁。

无目标 `kat inspect` 仍只读取 manifest，不批量读取 Source Guides。

Rust 对 Runtime DTO 使用 `deny_unknown_fields`，再把 manifest 与 Runtime 结果组合为：

```json
{
  "name": "pack-name",
  "title": "...",
  "description": "...",
  "owner": "...",
  "source_guide": "...或 null",
  "sources": [],
  "workflows": []
}
```

## 4. 唯一 Dataset 与 Binding 模型

唯一受支持的 Dataset 使用以下 KAT 私有布局：

```text
<dataset>/
  .kat-dataset
  bindings.json
  sources/
    <pack>/
      <source>/
        tables/
          <table>.parquet
```

`.kat-dataset` 必须是空普通文件。`bindings.json` 使用严格 tagged union：

```json
{
  "bindings": [
    {
      "pack": "example-pack",
      "source": "raw_smaps",
      "kind": "external",
      "arguments": ["--files", "capture.log"],
      "working_directory": "C:\\work"
    },
    {
      "pack": "other-pack",
      "source": "facts",
      "kind": "materialized",
      "arguments": ["--input", "capture.htrace"],
      "working_directory": "C:\\work",
      "tables": ["events", "processes"]
    }
  ]
}
```

External Binding 精确包含 `pack`、`source`、`kind`、`arguments` 与 `working_directory`；Materialized Source 另含 `tables`，并保留 `arguments` 与 `working_directory` 作为 REDO recipe。Metadata 不保存 PACK path、代码、版本、哈希或补全后的默认参数；recipe 不是权威 provenance 或一致性保证。同一 `(pack, source)` 重复即 Dataset 无效。写入时 bindings 与 tables 排序，读取不依赖原始顺序。

`working_directory` 必须是绝对 Unicode path，读取时不要求它仍然存在。Materialized `tables` 必须非空、唯一且名称合法，每张表恰好使用 `tables/<name>.parquet` 普通文件；同名目录明确无效。KAT 只负责路径边界、表名、顶层字段唯一性与 Parquet metadata 可读性检查。未被 metadata 引用的残留文件不形成 Binding，也不进入查询面。

合法空 Dataset 的 metadata 是 `{"bindings":[]}`，但不增加 `kat create-dataset`。目标不存在时，`kat bind` 或 `kat materialize` 可以创建它；目标已经是普通目录或文件时明确拒绝。

旧扁平 `.kat-dataset + tables/*.parquet` 与旧 `catalog.json` Dataset 不自动推断 PACK/Source，也不自动迁移；已经存在的旧 Dataset 由 inspection 拒绝，并提示通过当前来源和配置 REDO。

## 5. Rust 与 Python 之间的 Dataset DTO

Runtime Dataset 引用精确使用：

```json
{
  "path": "C:\\dataset",
  "sources": [
    {
      "pack": "example-pack",
      "source": "raw_smaps",
      "kind": "external",
      "arguments": ["--files", "capture.log"],
      "working_directory": "C:\\work"
    },
    {
      "pack": "other-pack",
      "source": "facts",
      "kind": "materialized",
      "arguments": ["--file", "capture.parquet"],
      "working_directory": "C:\\work",
      "tables": [
        {"name": "events", "path": "C:\\dataset\\sources\\other-pack\\facts\\tables\\events.parquet"}
      ]
    }
  ]
}
```

`path` 与 Materialized Source 中各 table 的 path 都是 canonical absolute paths；table path 必须指向一个普通文件并留在对应 `(pack, source)` 空间内。`sources` 按 pack/source 排序。Query 与 run/test request 都携带全部 Bindings，但 Runtime 的 Materialized DTO 只携带 table paths，不投影其 recipe；生产 request 不携带临时 Source override。

公共 `kat inspect --dataset` 精确返回：

```json
{
  "path": "C:\\dataset",
  "sources": [
    {"pack": "example-pack", "source": "raw_smaps", "kind": "external"},
    {
      "pack": "other-pack",
      "source": "facts",
      "kind": "materialized",
      "tables": [
        {
          "name": "events",
          "columns": [
            {"name": "ts", "type": "Int64", "nullable": false}
          ]
        }
      ]
    }
  ]
}
```

该操作完全由 Rust 执行，不执行 PACK discovery、不调用 Provider，也不创建 Operation log。任一管理 metadata 或受管理 Parquet metadata 无效时，整个 inspection 失败。

## 6. DataFusion catalog/schema 与延迟 Source Resolution

每次 run、query、materialize 或 `kat_run` 调用都创建独立 Source operation。Runtime 原样使用 PACK identity 建立 DataFusion catalog，原样使用 Source name 建立 schema；执行 Workflow 时，其 PACK 是 current catalog。该 catalog 的 default schema 使用 Source name 规则无法命中的私有名称 `__kat_workflow__`，其中不注册事实表。没有 current PACK 的 Query 同样使用 PACK identity 规则无法命中的私有 current catalog，避免与合法 Dataset catalog 冲突。每个 session 继续启用 DataFusion 自带的 `information_schema`，以保留 `SHOW TABLES` 等既有查询行为；Source authoring 因此在 inspection 阶段拒绝同名 Source。KAT 不提供 `dataset` schema 或未限定 alias；合法的 `public` Source 仍只能通过 `public.<table>` 访问。

锁定的 DataFusion Python 54 已实测：`SessionContext.register_catalog_provider` 注册纯 Python Provider 时不会执行回调；解析三段名时依次调用 `CatalogProvider.schema()` 与 `SchemaProvider.table()`。Runtime 因此为每个 PACK 注册一个惰性 catalog；创建 Source operation 或注册 catalog 时不执行 Source Entry，只在某个 Source schema 首次被请求时才按 `(pack, source)` 解析 Binding 并调用必要的 Entry。

Source Entry 成功返回后，Runtime 按 DataFusion 的原生边界处理三种形态：公开 `Schema` 转为其底层 `RawSchema`；官方 FFI exporter 在绑定当前 session 的内存 catalog 中执行一次，并归一化为原生 Provider；纯 Python `SchemaProvider` 则经过兼容适配器。整个归一化过程都处于 Source Resolution 的成功或失败缓存边界内，适配时不读取 `table_names`。不能在 DataFusion 的 Tokio Provider 回调中调用公开 `Schema.table()`，否则 DataFusion 54 会因嵌套 runtime 而崩溃。

DataFusion 54 会吞掉 `CatalogProvider.schema()` 直接抛出的 Python 异常，FFI 导出若留到该回调返回后执行，也只会留下模糊的 table-not-found。Runtime 因此先在可捕获的归一化边界完成 FFI 导出，再在 catalog 边界捕获 Source 解析失败，返回一个保存原始异常链的失败 Provider，由其 `table()` 报错。KAT 不靠扫描 SQL、匹配字符串错误或提前实例化全部来源来绕过这一行为。

Resolution 规则如下：

- 任意 PACK External Binding：首次请求该 schema 中的表时，从当次 discovery 唯一选择对应 PACK，在私有 module namespace 中按保存的工作目录编译 argv，调用 Source Entry 并缓存 Provider；
- 任意 PACK Materialized Source：首次请求表时打开对应本地 Parquet TableProvider，不读取 recipe；
- 已声明但没有 Binding 的当前 PACK Source：首次请求表时，错误消息包含完整的 PACK/Source 身份，并提示选择 Dataset，或先 bind/materialize；
- PACK 已变化而 External Binding 找不到匹配 Entry：只在请求该 Source 时失败。

每个 `(pack, source)` 在一次 operation 中至多解析一次；成功取得的 Provider 和解析失败都会缓存。不同 schemas 中的同名表互不影响。Materialized schema 也只在请求具体表时打开对应 Parquet Provider，不在 session 初始化阶段打开所有表。`kat materialize` 需要枚举全表或选定表时，则在 DataFusion Provider 回调之外完成枚举适配。

DataFusion 54 还有一个已实测的上游缺陷：Python API 把 `SchemaProvider.table_names()` 声明为方法，Rust bridge 却把 `table_names` 当成 Sequence 属性读取。KAT 只对纯 Python `SchemaProvider` 做这层兼容：既接受方法形式，也接受属性形式，再向 DataFusion 暴露排序后的属性形式。原生 `Schema` 与官方 FFI exporter 不经过该适配器。

当前依赖精确锁定 `datafusion==54.0.0`。升级到 55 或更高版本前，必须验证 Source 返回官方 `Schema` 与 Hitrace FFI SchemaProvider 时仍能惰性完成三段式查询，且不触发嵌套 runtime。如果新版已能在惰性 catalog 中原生解包公开 `Schema`，应从唯一适配 helper 中删除 `_raw_schema` 兼容分支，不保留两套路径。Hitrace production exporter 的查询、物化和重复导出证据由后续来源 SDD 强制提供，不能用伪造 capsule 代替。

## 7. Arrow reader helper 与 Source staging

公共 helper 固定为：

```python
from collections.abc import Callable, Mapping
import pyarrow as pa
from datafusion.catalog import SchemaProvider

def schema_from_readers(
    factories: Mapping[str, Callable[[], pa.RecordBatchReader]],
) -> SchemaProvider: ...
```

构造时复制 mapping，并校验 table name 与 factory callable。Mapping 可以为空，但全表 materialize 会因零表失败。枚举名称与检查名称不会调用 factory；首次 `table(name)` 才调用对应零参数 factory。Factory 必须返回 `pyarrow.RecordBatchReader`。

Runtime 按 RecordBatch 增量写入一张 staging Parquet。即使 reader 为空，也依据 `reader.schema` 生成合法空表；完整写入后，才通过官方 PyArrow Dataset 路径构造 TableProvider 并交给 DataFusion。这避免了在 DataFusion 的 Tokio 回调中调用 `SessionContext.read_parquet` 而再次进入 runtime。同一表在每次 operation 中只调用一次 factory，并以每表锁避免并发重复。Factory、读取或写入失败时，KAT 删除未完成文件，缓存该失败，并使当前查询失败。

不能使用 `SessionContext.read_batches(generator)` 或 `from_arrow(RecordBatchReader)` 代替 staging：DataFusion 54 的本地实测表明，两者都会在返回 DataFrame 前先消费全部 batch。输出侧的 `DataFrame.execute_stream()` 和 DataFusion Parquet writer 才是增量执行通道。

Source staging 以 operation 为生命周期：一次 `kat run`、一次 `kat materialize` 或一次 `kat_run` fixture 调用各自拥有唯一空临时目录。Runtime 通过私有 `ContextVar` 在 Source operation 中向 helper 提供 session 与 staging handle，不增加 SourceContext。Helper 在 operation 之外调用时明确失败。Operation 结束后尽力删除；崩溃残留不得跨 operation 复用。

该 mapping 只是 helper 输入，不是 Source Entry 的第二种返回类型。纯 Python provider 看不到 projection/filter/limit；需要动态下推时，作者继续复用原设施或 Rust/FFI TableProvider。

## 8. CLI 与私有 Runtime 操作

### 8.1 `kat bind`

调用形式为：

```text
kat bind --pack <pack> --source <source> --dataset <path>
  [--replace] [--pack-dir <directory> ...] [-- <source arguments>]
```

Rust 先完成 PACK discovery、Dataset 目标与 Binding 冲突的只读机械检查，再发送 `bind_source` request。Runtime 只执行 Source 扫描、Guide 门禁、目标 Source 选择和参数编译，不调用 Entry。Runtime 成功后，Rust 才保存原始 argv 与规范化后的当前工作目录。目标不存在时此刻才创建 Dataset；已有 Binding 默认拒绝，`--replace` 完整替换，并且不继承任何旧 argv。

公共成功结果精确为：

```json
{"path":"C:\\dataset","pack":"example-pack","source":"raw_smaps","kind":"external"}
```

### 8.2 `kat materialize`

调用形式为：

```text
kat materialize --pack <pack> --source <source> --dataset <path>
  [--table <table> ...] [--replace]
  [--pack-dir <directory> ...] [-- <source arguments>]
```

Rust 在执行第三方代码前完成 Dataset 目标、Binding 冲突、`--replace`、PACK discovery 与表名等机械检查。Source arguments 按固定顺序选择：

1. 本次 argv 非空：使用本次 argv 和当前工作目录；
2. 没有本次 argv，且当前是 External Binding：重放保存的 argv 和保存的工作目录；
3. 没有本次 argv，且没有 Binding：使用空 argv 和当前工作目录；
4. 没有本次 argv，且当前是 Materialized Binding：重放其中保存的 argv 和工作目录；查询仍只使用 Materialized tables。

已有任一 Binding 时，即使本次提供了新 argv，也必须显式 `--replace`。

Runtime 执行 Guide 门禁并校验 Source contract，调用 Entry，并通过内部 adapter 注册 Provider。未提供 `--table` 时，在操作开始时枚举一次全部表，并拒绝零表；提供时先排序去重，再拒绝未知表。每张表用 DataFusion writer 完整写入私有 export 目录中的单个 Parquet 文件；成功返回排序后的表名。Rust 对 export 做与 Dataset inspection 相同的机械验证，再发布到目标 Source 空间并更新 `bindings.json`。

私有 export 仅是实现细节，不提供回滚、保旧或崩溃恢复保证。进入 Provider 或写入阶段后的任何失败都可能导致目标 Source 空间或 Dataset 无效。

公共成功结果精确为：

```json
{
  "path":"C:\\dataset",
  "pack":"example-pack",
  "source":"raw_smaps",
  "kind":"materialized",
  "tables":["mappings","snapshots"]
}
```

### 8.3 `kat run`

CLI 形态保持不变。`--dataset` 仍可选；显式 Dataset 在启动 Runtime 前完成机械验证。Run Manifest 仍只保存 canonical Dataset path，不保存 Bindings、Source arguments 或 staging。

### 8.4 `kat query`

`--run` 与 `--dataset` 属于同一个 Clap required group，必须且只能提供一个，并接受可重复的 `--pack-dir` candidates。Dataset 模式注册 External 与 Materialized Sources；Run 模式继续注册 `output.*`，并在关联 Dataset 当前有效时追加其中的全部 Bindings。纯 Materialized 查询不加载 PACK；External Source 只在 SQL 实际解析该 schema 时按当次 candidates 唯一发现和加载。默认搜索目录中的无关损坏或重名候选作为延迟诊断传给 Runtime，只有实际解析相应 External PACK 时才影响查询；每个显式 `--pack-dir` 则在 Runtime 启动前完成 canonicalize、目录和 manifest 校验，失败立即终止 Query。关联 Dataset 已失效时，Run 模式保留现有 `unavailable` 投影并继续查询 Outputs。

Dataset query 沿用现有公共结果形态：

```json
{
  "dataset":{"status":"available","path":"C:\\dataset"},
  "columns":[],
  "rows":[]
}
```

## 9. PACK test

`kat_run` 扩展为：

```python
kat_run(
    *,
    workflow: str,
    dataset: str | None = None,
    sources: dict[str, Sequence[str]] | None = None,
    arguments: Sequence[str] = (),
) -> dict[str, pa.Table]
```

`sources` key 必须是当前 PACK 的 Source name；value 不能是 `str` 或 `bytes`，每个 token 必须是 exact `str`。Fixture 不接受 Provider/SchemaProvider，也不允许覆盖其他 PACK 的 External Binding。显式 sources 在本次调用中覆盖 Test Dataset 的同名当前 PACK Binding，其他 Bindings 保留；编译、Entry 或 Provider 失败时不回退。每次调用拥有独立 Provider cache 与 staging，不修改 Test Dataset，也不写 Run Manifest。

Fixture Source argv 中的相对 Path 以 PACK root 为基准，避免测试结果依赖发起 `kat test` 的 shell 工作目录。`kat test` 仍只接受一个精确 `--pack-dir`。

## 10. Clock Source identity

原先不含来源身份的 clock conversion 改为：

```python
def convert_clock(
    self,
    clock_domain: Expr,
    clock_value: Expr,
    *,
    source: str,
    target_domain: str,
    pack: str | None = None,
) -> Expr: ...
```

`source` 必须是合法 Source name；`pack=None` 表示当前 Workflow PACK，跨 PACK 时显式传原始 PACK identity。Runtime 使用完整 `(pack, source)` 读取该 schema 的 `clock_domain` 与 `clock_snapshot`，不从同名表、注册顺序或 Dataset 内容猜测。`clock_domain` definitions 与 baseline readings 按该身份在本次 operation 内缓存，只在首次实际调用 clock conversion 时读取，因此也会遵守 Source 的延迟解析。既有 Schema、1 GHz、snapshot 0、溢出与 null 语义保持不变。

## 11. 普通日志纵向案例

新增 `kat-kernel` PACK 的 `raw_smaps` Source。SMAPS 在这里是普通日志案例，不进入平台类型系统，也不形成通用 Framer 协议。

```text
kat/packs/kat-kernel/
  pack.toml
  SOURCES.md
  sources/raw_smaps.py
  decoders/smaps.py
  analysis/process_memory.py
  workflows/process_memory_summary.py
  tests/fixtures/
  tests/test_raw_smaps.py
  tests/test_process_memory.py
```

Source Entry 接受可重复的 Path 参数；每个文件是一份已经采集的 SMAPS snapshot，不读取现场 `/proc`。输入顺序稳定分配 `snapshot_id`。Decoder 只理解一份原始 SMAPS chunk；将来若真实输入是“metadata + 多个 chunks”的巨大容器，只新增理解该容器的具体 Framer，并继续调用同一个 Decoder。

实现前评估过 Rust 生态的 `procfs-core`：它的 `MemoryMaps` 确实实现了 [`FromBufRead`](https://docs.rs/procfs-core/latest/procfs_core/trait.FromBufRead.html)，可以解析任意已打开的离线输入，但公开结果是 [`MemoryMaps(pub Vec<MemoryMap>)`](https://docs.rs/procfs/latest/procfs/process/struct.MemoryMaps.html)，会先保留整份映射集合。首切片只需要严格提取 `Size/Rss/Pss`，现有 Python decoder 能逐条产生 mapping、按固定批次交给随 Runtime 已提供的 PyArrow；为复用该 crate 而新增 Rust/Python adapter 与 native 发布面，成本高于这一小段领域解析。因此首切片不引入 `procfs-core`；若以后要求完整 SMAPS 字段覆盖，或 PACK 已有统一 native 依赖面，再重新比较迁移收益。

Source tables 为：

- `snapshots(snapshot_id, source_file)`；
- `mappings(snapshot_id, start_address, end_address, permissions, offset, device, inode, pathname, size_kib, rss_kib, pss_kib)`。

`snapshots` 与 `mappings` 分别通过 `schema_from_readers` 提供。Analysis 通过 `raw_smaps.mappings` 汇总 pathname 的 RSS/PSS；Workflow 只解释任务输入、调用 Analysis 并发布结果。

Fixture 至少覆盖：空文件、损坏 header 或 metric、多文件、一个 reader 的多个 RecordBatch、`kat_run(sources=...)`、bind 后 run、全表与子集 materialize、Dataset inspection、`query --dataset`，以及删除原始文件后 Materialized Source 仍可查询。

## 12. 顶层 Import 的退场

ADR-0063 与 [Hitrace Source SDD](2026-08-25-hitrace-source-and-import-removal.md) 先交付官方 DataFusion FFI Hitrace Source，再删除整个顶层 `kat import`。Deprecated Trace Streamer 与其两个预发布 PACK 直接删除，不迁移、不保留 alias。现有 Hitrace parser、domain facts 与合同测试继续作为 `kat-kernel/hitrace` 的实现底座；通用 `kat materialize` 不增加 Hitrace 特判。

## 13. 交付顺序

1. Source decorator、私有 Input Compiler 内核、inspection DTO，并删除 `required_tables`。
2. Dataset Binding 合同、Rust inspection 与 Runtime Dataset DTO。
3. `kat bind` 与 operation-specific PACK loading。
4. Lazy catalogs、run/test Source Resolution 与显式 Clock identity。
5. `schema_from_readers`、operation-scoped staging 与 `raw_smaps` PACK。
6. `kat materialize`、`kat query --dataset` 与 Run Query 的 Materialized Sources。
7. Hitrace FFI Source、跨 PACK External Query、REDO recipe 与单文件 Materialized tables。
8. 删除 Import/Trace Streamer，更新 README、KAT Skill、作者文档与全仓合同。

## 14. 验证证据

完成实现必须实际提供：

- `cargo fmt --check`；
- `cargo clippy --workspace --all-targets`；
- `cargo test --workspace`；
- 在 CPython 3.14 下构建当前 wheel，并运行全部 Python 测试；
- 真实 Bundled Host 的 inspect/bind/materialize/run/query/test suites；
- Source decorator、Compiler、Guide 与 exact DTO 单测；
- Dataset metadata 损坏、重复身份、同名目录、replace 与 legacy rejection 单测；
- 未使用的 Source 不调用 Entry 或 reader factory；
- 当前/跨 PACK External 与 Materialized Source 的惰性查询；
- 不同 Source schemas 中的同名表互不覆盖；
- reader factory 单次调用、空 reader、多 batch、失败失效与 staging 清理；
- `kat_run` override、隔离、不回退且不修改 Dataset；
- Clock `(pack, source)` 隔离；
- `raw_smaps` 完整纵向链路；
- Hitrace production FFI Source 可 query/materialize/re-query，Trace Streamer 与顶层 Import 已退场；
- 除本 SDD/ADR 的迁移说明和负向拒绝测试外，生产代码与当前使用文档不再使用 `required_tables` 或匿名 `dataset.*`。
