# Python PACK 详细合同

按[作者入口](../SKILL.md)完成常规复用、实现与交付。实现 Context 组合、来源物化、流式写入，或需要核对完整数据和 Guide 边界时读取对应章节；纯知识、空骨架和简单文案任务不读取本文件。

## 1. 使用当前作者与数据合同

Workflow 是普通模块顶层同步函数，由 `@kat.workflow(...)` 声明。Runtime 以 `ctx: Context` 和解析后的具名输入显式调用选中的函数。Context 提供当前 Session 共享的 `ctx.datasource_root`、当前候选执行私有的 `ctx.scratch_root`，以及线程安全的同步 `ctx.run(pack_name, workflow_name, /, **inputs)`；不提供 Session identity 或整个 Session 根。PACK 不从 Context 取得来源查询、Arrow 转换、时钟转换或隐式 relation catalog。

PACK 自有来源在顶层 `datasources/` 中使用普通 Python 模块和 Provider 类；复用公共 Provider 时可以省略这一层。Workflow 像调用其他 PACK 代码一样显式 import、构造并调用它们；KAT 不构造或包装 Provider。一次性解析、SQLite 或其他中间工作放在 `ctx.scratch_root`，执行结束后不得被后续 Workflow 当作输入。需要在同一 Session 复用的文件来源以明确参数的 `Path(source).stem` 作为 `ctx.datasource_root` 的直接子目录名；不扫描目录猜来源，也不附加 hash 或自动消歧。

Provider 必须拒绝空 Source stem、`.`、`..`、路径分隔符、控制字符、Windows 非法字符、尾随点或空格以及大小写不敏感的 Windows device name。PACK 自有且自行维护物化合同的 Provider 在目标存在时先用 `dp.open()` 打开，并校验允许的 relation 集合、每个实际 relation 的完整 columns/物理类型/nullability 与显式版本合同，只有目标不存在时才 decode 或 `dp.write()`。原生 decoder 使用每张 Parquet relation 的 Arrow Schema metadata `kat.materialization.version` 保存版本；这类 Provider 应与对应 `kat_datasource` 模块导出的 `MATERIALIZATION_VERSION_METADATA_KEY` 和 `MATERIALIZATION_VERSION` 比较，不把目录存在或 Schema 恰好相同当成版本兼容。自定义物化也必须定义并验证等价的稳定版本事实。

已经发布的同名物化打不开或合同不兼容时当前执行失败，不能删除、覆盖或原位修复；原始来源后来变化也不刷新当前 Session 的槽位。并发生产方各自完成 staging 后以 no-replace 发布，loser 打开并验证 winner，兼容则复用，否则失败且保留 winner。Session 内名称唯一性和大小写碰撞由调用方保证。

`kat-workflow` 与 `kat-datasource` 是 Payload 中两个独立的私有 wheel：

- `kat-workflow` 提供 `kat.workflow`、`kat.provider`、`kat.Context`、`kat.Duration`、`kat.WallClockTimestamp`、`kat.dataprovider` 和私有 Runtime；
- 平台原生 `kat-datasource` 提供窄的 `kat_datasource` 来源 API；它不依赖或重新导出 `kat`。

两个 wheel 随同一 KAT 版本原子安装，但 PACK 必须分别显式 import 所需模块，不能假设一个 distribution 会传递另一个。

Workflow 返回 `None` 表示无 Output；有输出时只能返回精确的 `dp.Table`，或一个非空普通 `dict[str, dp.Table]`。PyArrow Table、引擎惰性值、Table/dict 子类、空 Mapping 和混合值都不是 Output。Provider 的中间 Table、Catalog 和物化目录不会自动成为 Run Output。

## 2. 通过 `ctx.run()` 组合 Workflow

需要复用、inspection 和测试的固定组合仍写成普通 Python Workflow。每次调用必须显式提供完整 PACK name 与 Workflow name，即使目标位于当前 PACK；两个路由参数仅限位置，目标 Workflow 的输入全部使用关键字。Context 只从顶层 `kat run` 或 `kat test` 已确定的 discovery roots 中寻找目标，不能用路径参数增加搜索目录，也不能绕过执行边界直接调用另一个 Workflow 函数。Workflow declaration 不静态重复声明潜在子 Workflow、Output name 或 Output Schema；实际调用和子 Run 已发布的 Parquet 是唯一事实源。

顺序组合可以查询一个子 Catalog，从结果提取受支持的普通标量，再调用下一个 Workflow：

```python
import kat
from kat import dataprovider as dp


@kat.workflow(
    name="analyze-thread",
    description="先汇总 Trace，再检查 CPU 时间最高的线程。",
    parameters={"trace_path": "待分析 Trace 的路径。"},
    guide="workflows/analyze-thread.md",
)
def analyze_thread(ctx: kat.Context, *, trace_path: str):
    summary = ctx.run("trace-pack", "summarize", trace_path=trace_path)
    selected = dp.DataFusionProvider(catalog=summary).query(
        "SELECT thread_id FROM main ORDER BY cpu_ns DESC LIMIT 1"
    )
    thread_id = selected.to_arrow()["thread_id"][0].as_py()

    detail = ctx.run(
        "thread-pack",
        "inspect",
        trace_path=trace_path,
        thread_id=thread_id,
    )
    return dp.DataFusionProvider(catalog=detail).query(
        "SELECT * FROM findings WHERE severity >= 2"
    )
```

`ctx.run()` 只在子 Run 完整发布后返回只读 `dp.Catalog`；无输出时 `catalog.tables == ()`。子 Workflow 返回单个 Table 时 relation name 固定为 `main`；返回字典时 relation name 保留各 Output key，实际的 `catalog.tables` 是调用方可依赖的名称合同。Catalog 直接引用已经发布的 Parquet，不 eagerly 复制整个子 Output；只有交给 `dp.DataFusionProvider(catalog=...)` 查询时才读取所需数据。`dp.Catalog` 不能作为下一次 `ctx.run()` 的输入，也不能成为父 Workflow 的 Run Output。

嵌套输入必须是与目标标注严格对应的精确 `str`、有符号 64 位 `int`、有限 `float`、`bool`、允许的字符串 `Literal`、`kat.Duration`、`kat.WallClockTimestamp`，或仅供 Optional 参数使用的 `None`。不执行 CLI 字符串转换，不把 `bool` 当作 `int` 或把 `int` 当作 `float`；省略的输入由目标 Input Compiler 应用默认值。`dp.Table` 与 `dp.Catalog` 都不是 Workflow input value。

多个子 Catalog 先分别查询、投影或聚合成较小的 Table，再通过 `dp.DataFusionProvider(tables={...})` 融合；首版不直接联邦查询多个 Catalog。确有独立子调用时可以使用 Python 标准线程能力，但父入口返回前必须等待自己启动的全部工作。推荐使用 `concurrent.futures` 并调用每个 `Future.result()`，使线程错误按普通 Python 语义传播；单独 `Thread.join()` 不会重抛工作线程异常。Context 关闭后新的调用同步失败，父入口返回时仍有已登记调用也不会发布父 Run。

活动调用链中再次出现相同 `(PACK, Workflow)` 会抛出 `kat.RunError`，已经完成后的顺序重复调用合法并形成新的子 Run。`kat.RunError` 只有异常类型稳定，消息不能解析，也不提供 phase、Session ID、Run ID、路径或是否已发布字段；即使父级捕获并降级，调用也可能已经留下成功 Run 或成功后代，因此主动重试始终按可能产生重复 Run 的全新执行处理。

组合 Workflow 若只固化调用与 Guide 汇总，可以返回 `None`（含隐式 return）。仍发布普通 Run Manifest，`outputs: {}` 和实际直接 `child_runs`；空 dict 返回不合法，零行 Table 仍保留自己的 Schema：

```python
def collect_evidence(ctx: kat.Context, *, trace_path: str):
    ctx.run("cpu-pack", "analyze", trace_path=trace_path)
    ctx.run("io-pack", "analyze", trace_path=trace_path)
    # 隐式 None，无需占位表。
```

`kat_run` 在每测试独立临时 Session 内通过真实独立 Runtime 与生产执行/发布核心运行 Workflow。同测试多次调用共享来源物化、使用不同 scratch；返回便利的 `dict[str, pyarrow.Table]`，无输出时为 `{}`。pytest 的模块 monkeypatch 不会影响 Workflow；普通 helper 单测仍可 monkeypatch。被测 PACK 固定来自 `--pack-dir`，跨 PACK 子调用只从正常默认或已安装 roots 发现，不增加 sibling checkout 参数。

当前任务需要源码侧组合示例且已明确仓库根时，可读取 `<repository-root>/examples/packs/workflow-composition`；它不是随包资源，不为寻找该可选示例扩大目录搜索。

## 3. 组织和引用 guide

面向 AI 的知识只保留当前任务判断必需的信息，按以下归属编写和核对：

- 脚本完成可重复验证的输入校验、采集、转换、计算及必要子调用；固定步骤不能交给 AI 按 Guide 补做。输出提供所需结构化事实与可追溯证据，明细按需查询，不将完整日志或执行叙述作为分析上下文。
- 执行前必须知道的能力用途、输入语义和环境前提写入 Workflow description 与 parameters，不能只藏在执行后才读取的 Guide 中。优先补齐现有描述，不另建前置条件框架。
- Workflow Guide 解释容易误读或影响结论的输出：指标含义、单位、统计口径、适用范围，以及零值、空结果和缺失值的含义。计算定义、聚合或关联对证据范围的影响属于结果语义；不复述函数调用、数据搬运和脚本执行顺序。
- 逐项对照实现核准输出解释；没有来源依据时不编造单位、阈值或覆盖范围。实际表与列以 Runtime inventory 为准，Guide 不复制完整 Schema、参数清单或源码。
- 推理知识保留判断依据、替代解释、结论局限及由证据触发的下一步。仅使用已发布 Output 或适用 Workflow 补证据；缺少能力时说明缺口，不引导分析 AI 直接访问 Provider、中间数据库或私有文件。
- Provider Guide 面向作者，保留必要的接入、SQL、Schema 和来源语义；复用公共 Provider 时使用其已有 Guide，不另建重复的接入教程。无额外输出解释或推理知识的 Workflow 可以不写 Guide，不强制章节或凑齐模板。

Guide 的 decorator 路径相对 PACK 的 `knowledge/`，例如 `workflows/memory-summary.md`。声明必须指向知识目录内已有、非空、有效 UTF-8 的普通 `.md` 文件；绝对路径、路径穿越和解析后逃逸 `knowledge/` 都会被拒绝。普通 README 不要求关联为 Guide，框架也不限制 Markdown 的章节和写法。

Workflow list inspection 校验导入、声明和名称唯一性，只返回 `name`、`description`；detail 返回用途与参数，两者均不读取 Guide 文件。只有执行选定 Workflow 时，程序才在业务执行前读取、验证并捕获它的 Guide，随成功 Run 保存；顶层成功 Response 返回自身 `guide` 字符串，未声明时为 `null`。子 Guide 经 `inspect run` 按需读取，PACK 变更不改写历史快照。Provider inspection 仍校验所选范围的 Guide，detail 返回非空字符串。分析 Agent 使用公开字段，不自行组合路径或实现 include；作者检查本次获授权编辑的 Guide 文件与装饰器关联。

Workflow guide 只解释声明它的 Workflow 所发布的 Run，不自动继承、拼接或替代子 Guide。父 Guide 应说明自身 Output 的单位、范围、关键限制和足够作结论的条件。需要子结论时，说明哪些子证据支持哪个父判断，以及缺失、矛盾或重叠证据如何影响汇总；引导分析 Skill 沿真实 `child_runs` 选择相关子 Run，分别使用自己的 Guide 和最少证据，再回到父级汇总。父证据足够时不要求解释全部叶子；这是自由 Markdown 指导，不是新的可执行语法。

无输出父 Workflow 要说明如何综合必要子证据，不能把编排成功解释为没有问题。子结果继续遵守各自 Guide 的语义边界，复用同一证据时不要重复计数。缺省 Guide 不要求独立解释，也不需要为保存分析而补占位 Guide、固定章节或独立子报告模板。

Guide 建议在解释阶段继续运行的 Workflow 会在当前 Analysis Session 中形成新的独立根 Run，不会事后加入或修改已经发布的父 `child_runs`。若某个子调用是父结果成立所必需的确定性步骤，必须把它写入父 Workflow 的 Python 控制流，不能依赖 Guide 追认调用关系。

后续取证建议说明目的和参数来源；分析 Skill 将必要原因与解释直接保存为 Analysis Record 的正文，程序自动维护 Run Guide 快照与报告树。Guide 作者无需给 Markdown 增加记录字段或保存命令，也不让 Python Workflow 写 AI 解释。更新 PACK 不会改写已有快照；本合同不提供替换旧 Run Guide 的入口。

## 4. Provider inspection 的执行边界

不带 `--pack` 的 Provider inspection 直接读取平台公共声明与随 wheel 安装的 guide，不扫描或导入 PACK，也不构造 Provider。带 `--pack` 的 Provider inspection 会递归导入所选 PACK 顶层 `datasources/` 下的普通 Python 模块，并收集由各模块自身定义且经过 `@kat.provider` 装饰的类。一个模块可以声明零个、一个或多个 Provider；从其他模块 import 的声明不会重复计数。

因此 `datasources/` 必须 import-safe：模块导入可以定义类和纯元数据，但不应建立数据库连接、读取凭据、解析输入、启动进程或执行查询。KAT inspection 也不会实例化 Provider 或调用其业务方法。任一导入错误、非法声明、重名或 guide 错误会使本次 inspection 原子失败，不返回部分 Provider 列表。

这是运行时 Python 发现，不是静态 AST 扫描。只有 Provider inspection 扫描完整 `datasources/`；Workflow inspection 不触发它。

## 5. 显式来源解码与融合

原生 Hitrace 解码由 PACK 显式调用：

```python
from pathlib import Path
import kat
from kat import dataprovider as dp
from kat_datasource import hitrace


@kat.workflow(
    name="summarize-trace",
    description="解码并汇总一份 Hitrace。",
    parameters={"source_path": "Hitrace source path."},
)
def summarize_trace(ctx: kat.Context, *, source_path: str):
    relations = ctx.scratch_root / "relations"
    hitrace.decode(Path(source_path), relations)
    catalog = dp.open(root=relations)
    return dp.DataFusionProvider(catalog=catalog).query("SELECT ...")
```

`hitrace.decode()` 要求 destination 尚不存在；成功后 destination 的直接子级只含扁平具名 Parquet relation，并返回不可变 `DecodeReport`，列出 unsupported plugin 和 section type。上例是无需跨 Run 复用的一次性工作，因此使用 scratch。要复用时改用经过上述合法性检查的 `ctx.datasource_root / Path(source_path).stem`，命中先 open 与验证，未命中才 decode；失败时不要把残留路径、部分 relation 或 unsupported report 当作成功。

来源查询已经完整取得少量 Python rows 时，可以用显式物理 Schema 一次形成不可变 Table：

```python
import pyarrow as pa

schema = pa.schema(
    [
        pa.field("event_type", pa.string(), nullable=False),
        pa.field("event_count", pa.int64(), nullable=False),
    ]
)
result = dp.Table.from_rows(rows, schema=schema)
```

`from_rows()` 会立即消费 rows，严格校验字段、nullability 和物理类型并完成 Arrow 转换；它
适合已经完成的 eager 查询结果，不是追加构建器或落盘入口。已有 `pyarrow.Table` 则继续用
`Table.from_arrow()` 保留其 Arrow backing。

Datasource `Schema` 只接受下列逻辑类型。用 `T | None` 声明 nullable 列；裸 `T` 拒绝
`None`。每个非空值必须是表中所列的精确 Python 类型，不接受子类或隐式转换。

| Schema 类型 | Parquet 前的 Arrow 类型 | 关键拒绝规则 |
| --- | --- | --- |
| `bool` | `bool` | 只接受精确 `bool` |
| `int` | `int64` | 拒绝 `bool` 及有符号 64 位范围外的值 |
| `float` | `float64` | 只接受精确 `float` |
| `str` | `string` | 拒绝无法编码为 UTF-8 的文本 |
| `bytes` | `binary` | 只接受精确 `bytes` |
| `datetime` | `timestamp[ns, tz=UTC]` | 必须带有效 UTC offset；规范化到 UTC 后须在有符号 64 位纳秒范围内 |
| `Decimal` | `decimal128(38, 18)` | 必须有限，且能在不舍入的前提下缩放到 18 位小数并满足 38 位精度 |

除此之外的类型、空的 `Schema`、空列定义以及与声明不完全一致的行字段都会被拒绝。
`Schema` 构造后不可变且不可继承；一次 `dp.write()` 使用其完整多 relation 声明作为固定事务合同。

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

DataFusion Provider 只看构造时显式传入的 relation，不发现来源 Provider、不触发远端查询，也不会自动取得同一 Analysis Session 的其他内容。完整可执行写法见随 Skill 发布的 [Data Provider reference PACK](examples/dataprovider-pack/README.md)。
