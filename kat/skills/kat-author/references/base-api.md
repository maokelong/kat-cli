# 框架作者 API

适用 wheel：`kat-workflow 0.1.1-rc.13`。

本文只列出 `kat-workflow` 面向 PACK 作者的公共能力。公共范围以 `kat.__all__` 和 `kat.dataprovider.__all__` 为准；使用 `import kat` 和 `from kat import dataprovider as dp`，不要导入下划线模块。

## `kat`

| API | 能力与必要用法 |
| --- | --- |
| `kat.Context` | KAT 为一次 Workflow 执行提供的上下文。`datasource_root` 用于当前 Session 共享的完整来源物化，`scratch_root` 用于本候选执行的临时中间文件；`run(pack_name, workflow_name, /, **inputs)` 同步组合另一个 Workflow 并返回 `dp.Catalog`。 |
| `kat.Duration` | Workflow 的非负时长输入类型，值是带 `ns`、`us`、`ms`、`s`、`min` 或 `h` 单位的十进制字符串。 |
| `kat.RunError` | `Context.run()` 无法交付可用 Output Catalog 时抛出的公开异常；消息不是程序接口，失败调用也不保证可安全重试。 |
| `kat.WallClockTimestamp` | Workflow 的绝对时间输入类型；接受带已知 UTC offset 的 RFC 3339 字符串并规范化为 `Z`。 |
| `kat.dataprovider` | 标准 eager Table、本地 Parquet 与 SQL 工具包；通常别名为 `dp`。 |
| `kat.provider(*, name, description, guide)` | 给普通 Provider 类附加 inspection 元数据并原样返回该类。它不定义基类、生命周期或查询接口；`guide` 相对 PACK 的 `knowledge/`。 |
| `kat.workflow(*, name, description, parameters=None, guide=None)` | 声明模块顶层同步 Workflow。函数首参必须是 `ctx: kat.Context`，其余参数都要有受支持标注与说明；返回 `None` 表示无 Outputs，也可以返回一个精确 `dp.Table`，或非空的精确 `dict[str, dp.Table]`。 |

Workflow 的名称、参数、默认值、Guide 和返回形状还会在 inspection 或运行时校验。组合现有 Workflow 前先做 list/detail inspection；`Context.run()` 的两个路由参数只用位置传入，目标输入只用关键字传入。

## `kat.dataprovider`

| API | 能力与必要用法 |
| --- | --- |
| `dp.Catalog` | 对已验证 Parquet relation 的只读视图；由 `dp.open()` 或 `Context.run()` 返回，`tables` 给出 relation 名称。 |
| `dp.DataFusionProvider(*, tables=None, catalog=None)` | 在具名 eager Table 和至多一个 Catalog 上执行本地 SQL；`query(sql, *, params=None)` 返回 `dp.Table`。至少提供一种输入，内存 relation 与 Catalog 名称不能重叠。 |
| `dp.Schema(tables)` | `dp.write()` 使用的不可变、有序逻辑 Schema。列类型为 `bool`、`int`、`float`、`str`、`bytes`、`datetime`、`Decimal` 或其中一种与 `None` 的 union。 |
| `dp.Table` | Arrow backing 的不可变 eager 单表值。使用 `Table.from_rows(rows, *, schema=...)` 或 `Table.from_arrow(table)` 创建；通过 `columns`、列下标、`to_rows()` 和 `to_arrow()` 读取。 |
| `dp.open(*, root=None, tables=None)` | 打开只读 Parquet Catalog；`root` 与 `tables` 必须且只能提供一个。`root` 发现目录下直接的 `*.parquet`，`tables` 显式绑定 relation 到文件或分片目录。 |
| `dp.publish_materialization(source, destination)` | 将同一文件系统上的完整来源目录以不覆盖目标的方式发布；调用方负责校验来源和处理竞争结果。 |
| `dp.write(schema, *, destination)` | 创建一次性流式 Parquet 写事务。必须在 `with` 内按 relation 取 sink 并调用 `append(**row)`；整个上下文正常退出后物化才成功，异常退出不发布，目标不得预先存在。 |

具体 SDK Provider、公共函数和其他官方能力不属于本文件；按 [开发前复用检查](reuse-check.md) 继续读取当前 `api.md`、候选 reference 与 inspection 结果。
