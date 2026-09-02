# PACK 创作与维护流程

## 1. 先定位 PACK，再按对象检查知识

用户指定已有 PACK 时，先调用裸 `kat inspect` 和需要时的精确 `--pack-dir`，从 manifest 概要定位它。裸 inspection 不加载 PACK Python，也不包含 Workflow 或 Provider 声明。

根据开发目标分别调用：

- `kat inspect workflow --pack <名称>`：了解 PACK 暴露的 Workflow；选中一个后追加 `--workflow <名称>` 读取参数合同和 analysis guide。
- `kat inspect provider --pack <名称>`：了解 PACK 已有的 Provider；选中一个后追加 `--provider <名称>` 读取代码位置和数据库、SQL、Schema 或接入 guide。

Workflow 和 Provider 是两个独立知识入口。开发 Workflow 时只在需要复用或修改数据能力时 inspect Provider；分析问题时不 inspect Provider。inspection 失败时按 Diagnostic 停止，不用静态源码扫描伪造公开声明。

新建 PACK 没有 KAT 任务契约内的 Issue 或 SDD 前置门；执行所在仓库的协作规范仍独立适用。

## 2. 声明可发现知识

Workflow 与 Provider 都用显式元数据供 Agent 索引：

```python
import kat


@kat.workflow(
    name="memory-summary",
    description="汇总进程内存变化并定位异常区间。",
    parameters={"input_path": "待分析输入的路径。"},
    guide="workflows/memory-summary.md",
)
def memory_summary(ctx: kat.Context, *, input_path: str):
    ...


@kat.provider(
    name="postgresql",
    description="查询远端 PostgreSQL 并返回可物化表。",
    guide="providers/postgresql.md",
)
class PostgreSQLProvider:
    ...
```

- `name` 是稳定索引；`description` 是列表筛选所需的明确用途。两者都必须显式声明，不使用 `title`，也不从 docstring 推导 description。
- Workflow `parameters` 是运行输入合同；`guide` 可选，内容指导结果解释、分析发散与下一步方向。
- Provider decorator 只附加 `name`、`description`、`guide` 元数据。它不要求基类、Protocol、注册表、固定方法或生命周期；Provider 类可以按数据源需要定义 `decode`、`query` 或其他能力，Workflow 显式调用它们。
- Provider `guide` 必填，用于说明数据库、可用 SQL、表与关系、Schema、解析或物化方式。它是作者知识，不是分析策略。
- Provider detail 的 `module` 与 `qualname` 由声明类机械取得，不能在 decorator 中覆盖。

## 3. 使用当前作者与数据合同

Workflow 是普通模块顶层同步函数，由 `@kat.workflow(...)` 声明。Runtime 以 `ctx: Context` 和解析后的具名输入显式调用选中的函数。Context 只提供 `ctx.datasource_root`；PACK 不从 Context 取得来源查询、Arrow 转换、时钟转换或隐式 relation catalog。

PACK 在顶层 `datasources/` 中拥有普通 Python 模块和 Provider 类。Workflow 像调用其他 PACK 代码一样显式 import、构造并调用它们；KAT 不构造或包装 Provider。文件 Provider 应在 `ctx.datasource_root` 下建立当前 Workflow 的临时 workspace，向 Provider 传普通路径，并在 eager Table 脱离来源后清理临时物化。

`kat-workflow` 与 `kat-datasource` 是 Payload 中两个独立的私有 wheel：

- `kat-workflow` 提供 `kat.workflow`、`kat.provider`、`kat.Context`、`kat.Duration`、`kat.WallClockTimestamp`、`kat.dataprovider` 和私有 Runtime；
- 平台原生 `kat-datasource` 提供窄的 `kat_datasource` 来源 API；它不依赖或重新导出 `kat`。

两个 wheel 随同一 KAT 版本原子安装，但 PACK 必须分别显式 import 所需模块，不能假设一个 distribution 会传递另一个。

Workflow 的 Run Output 只能是精确的 `dp.Table`，或一个非空普通 `dict[str, dp.Table]`。PyArrow Table、引擎惰性值、Table/dict 子类、空 Mapping 和混合值都不是 Output。Provider 的中间 Table、Catalog 和物化目录不会自动成为 Run Output。

## 4. 组织和引用 guide

一个 PACK 的作者知识统一放在顶层 `knowledge/`：Workflow guide 位于 `knowledge/workflows/`，Provider guide 位于 `knowledge/providers/`。框架不限制 Markdown 的章节和写法。

Decorator 中的 `guide` 是相对 `knowledge/` 的路径，例如 `providers/postgresql.md`。它必须指向 `knowledge/` 内已有、非空、有效 UTF-8 的普通 `.md` 文件；绝对路径、路径穿越和解析后逃逸 `knowledge/` 都会被拒绝。

List inspection 会校验全部声明及 guide，但只返回 `name`、`description`，不会把所有 Markdown 放进上下文。选中 detail 后，Runtime 才把对应文件按原样读成 Response 的 `guide` 字符串；Agent 直接使用该字段，不自行组合路径或实现 include。Workflow 未声明 guide 时 detail 返回 `null`；Provider guide 始终返回字符串。

Workflow guide 始终来自当前 PACK 版本，不快照进 Run。它是可信的分析策略，不是 Output
Schema、证据、结论或可执行控制流；实际 Run Output 的名称、列和类型始终是事实来源。当前
guide 与旧 Run 不兼容时，不得按 guide 猜测缺失数据，应忽略不适用条款或重新执行 Workflow。

## 5. Provider inspection 的执行边界

Provider inspection 会递归导入所选 PACK 顶层 `datasources/` 下的普通 Python 模块，并收集由各模块自身定义且经过 `@kat.provider` 装饰的类。一个模块可以声明零个、一个或多个 Provider；从其他模块 import 的声明不会重复计数。

因此 `datasources/` 必须 import-safe：模块导入可以定义类和纯元数据，但不应建立数据库连接、读取凭据、解析输入、启动进程或执行查询。KAT inspection 也不会实例化 Provider 或调用其业务方法。任一导入错误、非法声明、重名或 guide 错误会使本次 inspection 原子失败，不返回部分 Provider 列表。

这是运行时 Python 发现，不是静态 AST 扫描。只有 Provider inspection 扫描完整 `datasources/`；Workflow inspection 不触发它。

## 6. 显式来源解码与融合

原生 Hitrace 解码由 PACK 显式调用：

```python
from pathlib import Path
from tempfile import TemporaryDirectory

import kat
from kat import dataprovider as dp
from kat_datasource import hitrace


@kat.workflow(
    name="summarize-trace",
    description="解码并汇总一份 Hitrace。",
    parameters={"source_path": "Hitrace source path."},
)
def summarize_trace(ctx: kat.Context, *, source_path: str):
    with TemporaryDirectory(dir=ctx.datasource_root) as temporary:
        relations = Path(temporary) / "relations"
        hitrace.decode(Path(source_path), relations)
        catalog = dp.open(root=relations)
        return dp.DataFusionProvider(catalog=catalog).query("SELECT ...")
```

`hitrace.decode()` 要求 destination 尚不存在；成功后 destination 的直接子级只含扁平具名 Parquet relation，并返回不可变 `DecodeReport`，列出 unsupported plugin 和 section type。它不创建平台来源身份或持久状态。失败时不要把残留路径、部分 relation 或 unsupported report 当作成功。

自定义 Python Parser 需要处理大输入时，不要先把全部行累积进 eager Table。用
`dp.write()` 显式选择 relation，让调用线程继续解析、后台线程同时写 Parquet：

```python
schema = dp.Schema(
    {
        "events": {"timestamp": int, "payload": bytes},
        "capture": {"clock": str},
    }
)

with dp.write(schema, destination=relations) as sink:
    for event in parse_events(source):
        sink["events"].append(
            timestamp=event.timestamp,
            payload=event.payload,
        )
    sink["capture"].append(clock="boot")

catalog = dp.open(root=relations)
```

`append()` 返回只表示该行已经同步校验并被候选物化接纳；只有 `with` 正常退出才表示整个
目录成功发布。`destination` 的父目录必须存在、其自身必须不存在。批次和队列阈值由 Toolkit
管理；该入口是一次性只写过程，不提供处理中查询，也不替代查询结果与 Run Output 使用的
不可变 eager `dp.Table`。`dp.write()` 是唯一公共 Datasource 物化入口；`Schema` 只声明
多 relation 结构，不创建 Table，`Table` 也不提供逐行 append。

`dp.open(root=...)` 发现一个 flat Parquet 目录；`dp.open(tables=...)` 绑定明确的 relation 路径。需要跨来源融合时，Workflow 先显式调用每个 Datasource Provider 得到 eager Table 或 Catalog，再把具名内存 Table 和至多一个磁盘 Catalog 交给普通 DataFusion Provider：

```python
local = dp.open(tables={"placement": placement_path})
result = dp.DataFusionProvider(
    tables={"telemetry": telemetry_provider.query(...)},
    catalog=local,
).query("SELECT ...")
```

DataFusion Provider 只看构造时显式传入的 relation，不发现来源 Provider、不触发远端查询，也没有跨 Workflow Session。完整可执行写法见随 Skill 发布的 [Data Provider reference PACK](examples/dataprovider-pack/README.md)。

## 7. 实施并验证已授权变更

理解、检查和测试默认只读。只有用户明确要求创建、修改或修复时才写入指定 PACK，并保持最小切片。编写 Provider 时优先复用 KAT 已公开的数据表、物化和查询能力；具体用法以已选 Provider guide、公共库接口和 reference PACK 为准，不发明框架约束。

写入后按变更面验证：

1. 重新执行对应 Workflow 或 Provider list inspection，确认所有声明与 guide 都能完整校验。
2. 对新增或修改的声明执行 detail inspection，核对精确公开字段和 guide 内容。
3. 运行适用的 `kat test --pack-dir ...`；fixture 用普通来源文件、Provider 配置和临时路径构造生产边界。成功 `result.summary` 是测试结论，失败时使用 Response、报告和日志定位。
4. 交付变更摘要、受影响文件、inspection/test 证据和仍存限制。

“诊断失败”本身不授权修复。无法在已有授权和事实下继续时，按 [result-contract.md](result-contract.md) 交付最小下一步。
