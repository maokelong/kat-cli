# KAT External PACK 开发指导

本文面向第一次接触 KAT 的开发者和 AI，目标是让一个新的 External PACK 从零走通
`inspect → test → run → query`，同时不依赖对 kat-cli 仓库历史的了解。

- 交付跟踪：[Issue #222](https://github.com/maokelong/kat-cli/issues/222)
- 公共 common 与 PostgreSQL 能力：[Issue #223](https://github.com/maokelong/kat-cli/issues/223)
- 事实基线：以本文随附的 KAT Skill/Release 版本为准
- 适用对象：不随 KAT Skill 发布、通过 `--pack-dir` 加载的 External PACK
- PostgreSQL 专用案例：[`postgresql-pack-development.md`](postgresql-pack-development.md)

KAT 仍处于 `0.1` 预发布阶段，PACK Authoring Interface 尚未承诺跨版本兼容。开始开发前，
应以实际要运行的那一版完整 KAT Skill 重新执行本文所有检查，而不是假设另一版的行为相同。

> 仓库流程：在 kat-cli 仓库内做非平凡变更时，先遵守根目录 `AGENTS.md`，建立 issue 和
> 轻量 SDD，明确问题、非目标、最小切片与验证方法。在独立 PACK 仓库中则遵守该仓库自己的
> 协作协议。

依据：`README.md:3-8`、`AGENTS.md:11-37`、
`docs/adr/0003-external-packs-are-a-first-class-deployment-unit.md:7-11`。

文中的“依据”路径是基线仓库内的一方事实索引；只拿到本文也可以按模板开发，拿到 kat-cli
checkout 时再用这些路径复核版本差异。

## 最快使用路径

如果目标是先让一个新 PACK 跑通，按以下顺序阅读和执行：

1. 填写下方“开发任务合同”，先决定 Workflow 使用 Dataset、直接读取外部来源，还是确有必要组合两者。
2. 从 KAT 维护者或发布渠道取得与目标版本匹配的完整 KAT Skill；缺少它就停止，不用 Cargo
   裸 CLI、系统 Python 或 venv 代替。
3. 复制第 4、5 节的 `pack.toml`、Workflow 和 `kat_run` 测试，先保持示例能力不变。
4. 按第 9 节执行 `inspect → test → run → query`，确认环境和发布链路完整。
5. 再把示例替换成真实领域逻辑，按第 6–8 节收紧 Interface、Dataset、Output 和依赖。
6. 最后按第 10、13 节补齐测试与交付证据。

开始写代码前，至少明确下面这些事实；缺失会实质改变实现时应先补齐，不要猜：

| 开发任务合同 | 必须明确的内容 |
| --- | --- |
| 交付身份 | PACK name、owner、每个 Workflow 要回答的具体问题 |
| 数据路径 | 是否需要 Dataset、哪些表对 Workflow SQL 可见、是否还读取外部来源 |
| 输入事实 | Dataset inspection 的表/列/类型，或外部协议、认证方式和规模边界 |
| Workflow 参数 | 名称、类型、默认值、choices；凭证不能作为参数 |
| Run Output | Output name、列、Arrow 类型、null/零行规则和最大合理规模 |
| Host 依赖 | 除标准库和目标 Skill 已交付库外的 import 是否已在实际 Bundled Host 验证 |
| 验收 | 正常、边界、失败用例，以及最终 `kat query` 要证明什么 |

## 1. 先建立正确的心智模型

### 1.1 核心对象

| 对象 | 它是什么 | PACK 作者需要关心什么 |
| --- | --- | --- |
| KAT Skill | KAT 的完整交付物，包含 CLI、Bundled Python Host、Runtime 和 Bundled PACK | 必须使用完整 Skill 中的 `kat`/`kat.exe`；Cargo 单独编出的 CLI 不能运行 Workflow |
| Source | 尚未进入 KAT 的原始输入，例如 trace 文件 | 只有 KAT 内置 Datasource 能通过 `kat import` 把它转换为 Dataset |
| Datasource | KAT 内置的 Source 读取与规范化实现 | 负责一次 `kat import`；External PACK 不能注册新的 Datasource |
| Dataset | KAT 管理的本地事实表目录，每张逻辑表对应一份受管理 Parquet | `required_tables` 控制 Workflow SQL 可见表；Dataset 对某些 PACK 可以完全不存在 |
| PACK | 一个自包含的领域分析发布单元 | 包含严格的 `pack.toml`、Workflow、可选 helper、测试和 Test Dataset |
| Workflow | PACK 的一个具名同步 Python 入口 | 接收 `ctx: kat.Context` 和已声明参数，返回一个或多个 DataFusion DataFrame |
| Context | 一次 Workflow 执行的受控能力边界 | 当前只有 `ctx.sql`、`ctx.from_arrow`、`ctx.convert_clock` 三个方法 |
| Public common | 与 KAT Platform 原子交付的公共代码能力 | 通过 `kat.common.*` 导入；首版包括 PostgreSQL 查询，不是可独立安装或跨 PACK import 的 common PACK |
| Run | 一次成功 Workflow 执行发布的记录 | 成功响应给出 `run_id` 和每个 Output 的列、行数；失败不会发布 Run |
| Run Output | Workflow 返回并由 KAT 物化的具名表 | 之后通过 `output.<output_name>` 查询；不是直接塞进终端的完整业务数据 |
| Output Query | 对已发布 Run 的只读 DataFusion SQL | `kat query` 不重新运行 PACK；作者流程应从 Run Output，以及可用时该 Run 引用的当前 Dataset 取证 |

PyArrow Table 是 Python 侧已经在内存中的列式表；DataFusion DataFrame 是惰性的查询/计算计划，
KAT 只接受后者作为 Workflow Output 并在发布 Run 前物化。外部适配器先得到 PyArrow Table 时，
必须通过 `ctx.from_arrow(table)` 转换。

依据：`README.md:23-28,96-143`、
`docs/adr/0020-dataset-writes-hide-physical-layout.md:7-13`、
`docs/adr/0032-workflow-execution-capabilities-require-explicit-context.md:7-23`、
`kat/platform/cli/src/run.rs:45-76`、`kat/platform/workflow/runtime/query.py:32-51`。

`kat query` 的当前实现还启用了 DataFusion URL table，受信任的本地 SQL 可以直接读取本机文件等
DataFusion 支持的来源。这不是来源隔离或安全沙箱。普通 PACK 开发闭环仍应使用
`output.*`/`dataset.*`，不要把 Query 偶然可读的外部来源变成 PACK 的隐式输入合同。

### 1.2 两条最常见的数据路径

Dataset 型 PACK：

```text
Source ──kat import──> Dataset tables
                           │
PACK Workflow ──required_tables + ctx.sql──> DataFrame(s)
                           │
                           └──> Run Output(s) ──kat query──> 调用方收窄的 JSON 结果
```

无 Dataset 型 PACK：

```text
Workflow 参数 / Bundled Host 已有客户端库 / 其他只读输入
                           │
                    Python / PyArrow Table
                           │ ctx.from_arrow
                           ▼
                       DataFrame ──> Run Output ──kat query──> 调用方收窄的 JSON 结果
```

最重要的判断是：

- Workflow 依赖 KAT Dataset 表时，声明真实的 `required_tables`，生产运行时传 `--dataset`。
- Workflow 不依赖 Dataset 时，声明 `required_tables=[]`，`kat run` 可以不传 Dataset，也不需要
  `kat import`。
- 实现并不禁止一个 Workflow 同时查询已授权 Dataset 表和读取外部来源；只有真实问题确实需要时
  才组合两者，并分别声明最小 `required_tables`、外部来源合同和失败边界。
- External PACK 不能注册新 Datasource、增加 `kat import` 等顶层子命令或新增 KAT 全局参数。
  Workflow 自己声明的参数仍会在 `kat run ... --` 之后形成该 Workflow 的 option。
  私有输入若不能由现有 Datasource 转换，要么在 Workflow 中使用 Bundled Host 已具备的库读取，
  要么修改并重新发布 KAT 本身。

依据：`docs/adr/0022-datasource-types-are-closed-and-bundled.md:5-9`、
`kat/platform/workflow/runtime/execution.py:126-147`。

## 2. 运行前提

### 2.1 必须拿到完整 KAT Skill

完整 KAT Skill 是本指南的外部输入制品，应由 KAT 维护者、受控开发包或发布渠道提供。受支持的
PACK 执行必须使用该 Skill 内的平台载荷：

```text
<skill>/
├── SKILL.md
└── scripts/targets/
    ├── linux-x86_64/
    │   ├── kat
    │   └── python/bin/python3
    └── windows-x86_64/
        ├── kat.exe
        └── python/python.exe
```

执行时只选择当前主机对应的 Payload；这不意味着可以把自行裁剪或拼装的部分目录称为正式完整
Skill。下面的情况都不是受支持的 PACK 开发环境：

- 只有 `cargo build -p kat-cli` 生成的 Rust 二进制；
- 只有源码 checkout，没有与 CLI 相邻的 Bundled Python Host；
- 用系统 Python、venv 或裸 `pytest` 直接代替 KAT Runtime；
- 把私有 Python launcher 当成独立 SDK 或公共 CLI。

如果只拿到源码、Cargo 二进制或不含相邻 Python Host 的目录，应停止并索取匹配目标 KAT 版本和
目标平台的完整 Skill。本文不把“自行拼出一套 Host”作为普通 PACK 开发步骤；确实需要新增 Host
第三方依赖时，按第 7.3 节把它作为独立交付前提处理。

KAT 从相邻载荷固定启动 Python Host，忽略系统 Python 和 `PYTHONPATH`。因此“本机
`python -c 'import kat'` 成功”或“裸 pytest 通过”都不能证明 PACK 可运行。

依据：`README.md:10-28`、
`docs/adr/0004-supported-execution-requires-the-bundled-python-host.md:5-13`、
`docs/adr/0047-current-pack-is-exposed-as-kat-pack.md:17`。

当前平台支持边界也必须写入验证结论：

- Linux 只支持 x86-64、glibc 2.28 或更高版本，是正式执行目标。
- Windows 10/11 x86-64 客户端目前仍是预发布候选，尚未完成 Issue #143 要求的干净客户端
  验收；Windows Server、Windows 7/8.1 不受支持。
- 其他系统、架构、libc 或版本应停止，而不是回退到 PATH 中的 `kat` 或系统 Python。

依据：`kat/skill/SKILL.md:69-76`、`README.md:54-58`。

### 2.2 本文命令约定

本文所有 shell 代码块都使用 PowerShell 7。先检查实际 Skill、CLI 和相邻 Host 路径；这里还不
解析 PACK 路径，因为第 5 节的示例文件尚未创建：

```powershell
$Skill = (Resolve-Path "C:\path\to\kat-skill").Path
$Kat = Join-Path $Skill "scripts\targets\windows-x86_64\kat.exe"
$HostPython = Join-Path $Skill "scripts\targets\windows-x86_64\python\python.exe"

foreach ($RequiredPath in @(
    (Join-Path $Skill "SKILL.md"),
    $Kat,
    $HostPython
)) {
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "Incomplete KAT Skill: missing $RequiredPath"
    }
}

& $Kat --help
if ($LASTEXITCODE -ne 0) { throw "KAT CLI is unavailable" }
```

Linux 若使用这些代码块，同样需要 PowerShell 7，并把 `$Kat`、`$HostPython` 分别改为
`<skill>/scripts/targets/linux-x86_64/kat` 和相邻的 `python/bin/python3`。使用 Bash 时需要翻译
shell 赋值、续行和 JSON 解析，但 KAT 参数的顺序不变。`--pack-dir` 必须指向直接包含
`pack.toml` 的精确 PACK 目录，不是它的父目录或 PACK 集合目录。

如需隔离开发产物，可为当前 shell 选择一个已经存在的绝对 Data Home：

```powershell
$DataHome = Join-Path `
    ([System.IO.Path]::GetTempPath()) `
    ("kat-pack-dev-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $DataHome | Out-Null
$env:KAT_DATA_HOME = $DataHome
```

该环境变量只影响由当前 shell 启动的进程。选中的目录必须已经存在、可访问且是绝对路径；
KAT 不展开 `~`、`%USERPROFILE%` 等缩写。Data Home 会保存日志、测试报告和 Run Output，可能
包含大量或敏感数据；不要提交或交付它，保留完验证证据后由开发者负责清理。

依据：`docs/adr/0003-external-packs-are-a-first-class-deployment-unit.md:9`、
`docs/adr/0060-file-and-environment-select-kat-data-home.md:7-25`、
`kat/platform/cli/src/lib.rs:612-632`。

## 3. PACK 的固定目录结构

最小可测试 PACK：

```text
example-values/
├── pack.toml
├── workflows/
│   └── build_summary.py
└── tests/
    └── test_build_summary.py
```

按需扩展：

```text
example-values/
├── pack.toml
├── workflows/                 # 可选；每个 .py 是一个 Workflow 入口
│   └── nested/example.py
├── helpers/                   # 可选；PACK 内共享的普通 Python
│   ├── __init__.py            # 可选
│   └── rules.py
├── tests/                     # 纯运行部署可省略；kat test 时必须存在并收集到测试
│   ├── conftest.py            # 可选，遵循 pytest 原生作用域
│   ├── test_example.py
│   └── datasets/              # 可选 Test Dataset 根
│       └── sample/            # 一个完整普通 KAT Dataset，选择器名为 sample
└── conftest.py                # 可选，仅 kat test 使用
```

结构约束：

- `workflows/` 下每个普通 `.py` 文件必须恰好定义并注册一个 module-top-level Workflow。
- Workflow `name` 必须是小写 ASCII kebab-case，并在整个 PACK 中唯一；`title` 去掉外围空白后
  必须非空。
- `workflows/` 任意层级都不能放 `__init__.py`。
- Workflow module segment 必须是有效且非 Python 关键字的标识符。
- 一个 Workflow 入口不能 import 另一个 Workflow 入口；共享代码放进 `helpers/`。
- helper 使用 `from kat.pack.helpers...` 导入；不要根据 PACK 目录名拼 Python package 名。
- `kat inspect --pack` 会在独立 worker 中实际 import 每个 Workflow module，模块顶层代码会执行。
  顶层只放 import、不可变常量、无状态定义和 Workflow 注册；不要在顶层连接数据库、读取凭据、
  发网络请求、写文件或根据当前环境决定 Interface。
- `tests/` 由 pytest 管理，不属于 `kat.pack`；生产代码的稳定身份是
  `kat.pack.workflows.*` 和 `kat.pack.helpers.*`。
- PACK 可以没有 Workflow，`inspect` 仍可能成功并返回空列表，但这不代表它已经提供可执行能力。

常用名称规则：

| 名称 | 规则 | 示例 |
| --- | --- | --- |
| PACK / Workflow | 小写 ASCII kebab-case | `example-values` / `build-summary` |
| Dataset table / Run Output | 小写 ASCII snake_case，避开 Windows 设备名 | `events` / `event_summary` |
| Python Workflow 参数 | 合法 Python 参数名；inspection 将 `_` 映射为 option 的 `-` | `item_count` → `--item-count` |
| Workflow 模块路径段 | Python 3.14 identifier，且不能是关键字 | `workflows/build_summary.py` |

依据：`docs/adr/0017-pack-source-layout-is-fixed.md:5-19`、
`docs/adr/0047-current-pack-is-exposed-as-kat-pack.md:5-17`、
`kat/platform/workflow/runtime/pack.py:162-257,273-335`。

## 4. `pack.toml`：恰好四个字段

```toml
name = "example-values"
title = "Example Values"
description = "Build a small typed summary for PACK development validation."
owner = "Example Team"
```

规则：

- 根级必须恰好包含 `name`、`title`、`description`、`owner` 四个 string。
- 不能增加 `[pack]` 包装，也不能增加 `version`、`dependencies` 或其他字段。
- `title`、`description`、`owner` 去掉外围空白后必须非空。
- `name` 使用小写 ASCII kebab-case，例如 `team-analysis`；不得使用 Windows 设备名
  `con`、`prn`、`aux`、`nul`、`com1`…`com9`、`lpt1`…`lpt9`。
- PACK identity 只来自 manifest 的 `name`，与目录名无关。
- 同一次 discovery 中两个不同目录声明相同 `name` 会整体失败，不存在覆盖优先级。

依据：`docs/adr/0007-pack-discovery-requires-a-static-manifest.md:5-9`、
`kat/platform/cli/src/pack_discovery.rs:60-67,119-157,210-256,501-521`。

## 5. 可直接复制的最小 PACK

这个模板故意不依赖 Dataset 或第三方库，用来先证明环境、PACK Interface、测试、Run 和 Query
全部连通。跑通后再把其中的领域逻辑替换为目标能力。

### 5.1 `workflows/build_summary.py`

```python
import kat
import pyarrow as pa


@kat.workflow(
    name="build-summary",
    title="Build Summary",
    required_tables=[],
    parameters={
        "label": "Label published in the summary.",
        "item_count": "Non-negative item count published in the summary.",
    },
)
def build_summary(
    ctx: kat.Context,
    label: str,
    item_count: int = 1,
):
    """Build one small typed summary row for end-to-end validation."""
    if item_count < 0:
        raise ValueError("item_count must be non-negative")

    table = pa.table(
        {
            "label": pa.array([label], type=pa.string()),
            "item_count": pa.array([item_count], type=pa.int64()),
        }
    )
    return {"summary": ctx.from_arrow(table)}
```

### 5.2 `tests/test_build_summary.py`

```python
import pyarrow as pa


def test_build_summary_runs_through_production_interface(kat_run):
    outputs = kat_run(
        workflow="build-summary",
        arguments=["--label", "demo", "--item-count", "2"],
    )

    assert set(outputs) == {"summary"}
    table = outputs["summary"]
    assert table.schema.equals(
        pa.schema(
            [
                pa.field("label", pa.string()),
                pa.field("item_count", pa.int64()),
            ]
        ),
        check_metadata=False,
    )
    assert table.to_pylist() == [{"label": "demo", "item_count": 2}]


def test_build_summary_uses_declared_default(kat_run):
    outputs = kat_run(
        workflow="build-summary",
        arguments=["--label", "defaulted"],
    )

    assert outputs["summary"].to_pylist() == [
        {"label": "defaulted", "item_count": 1}
    ]
```

`kat_run` 是 KAT pytest plugin 注入的生产执行 seam，返回
`dict[str, pyarrow.Table]`。无 Dataset Workflow 不传 `dataset`；需要 Test Dataset 时传的是
`tests/datasets/` 下的一级选择器名称，例如 `dataset="sample"`，不是路径。

依据：`kat/platform/workflow/runtime/testing.py:42-115`、
`examples/packs/postgresql-query/tests/test_live_postgresql.py:1-72`。

## 6. Workflow Interface 合同

### 6.1 装饰器与函数

每个入口的基本形状是：

```python
@kat.workflow(
    name="workflow-name",
    title="Human-readable title",
    required_tables=["table_name"],
    parameters={"argument_name": "Non-empty description."},
)
def workflow_function(ctx: kat.Context, argument_name: str):
    """Non-empty Workflow description exposed by inspection."""
    ...
```

必须满足：

- 装饰器使用具名参数，`name`、`title`、`required_tables` 必填，`parameters` 可省略。
- Workflow `name` 必须匹配小写 ASCII kebab-case，并在当前 PACK 内唯一；`title` 必须非空。
- 函数必须是 module-top-level 普通同步函数，不能是 lambda、async、generator 或嵌套函数。
- 第一个参数必须精确为 `ctx: kat.Context`，无默认值。
- docstring 必须非空；它会成为 inspection 中的 Workflow `description`。
- `parameters` 的 key 必须与 `ctx` 之后的函数参数一一对应，不能缺少也不能多出，说明不能为空。
- 用户参数可用 positional-or-keyword 或 keyword-only 形式；CLI 始终把它们公开为 option。
- Python 参数 `item_count` 映射为 CLI `--item-count`。
- KAT 不解析 Workflow return annotation；真实返回值才是执行合同。

成功应用 `@kat.workflow` 还不代表 Interface 合法；必须让 `kat inspect --pack` 完整加载并验证。

依据：`kat/platform/workflow/api/_workflow.py:92-170`、
`kat/platform/workflow/runtime/inspection.py:100-209`。

### 6.2 支持的参数类型

| Python annotation | CLI/inspection 类型 | 关键规则 |
| --- | --- | --- |
| `str` | `string` | 无默认值时必填 |
| `int` | `int64` | 限定有符号 64 位；Run Manifest 中有效值按十进制 string 保存 |
| `float` | `float64` | 必须有限，拒绝 NaN/Infinity |
| `bool` | `boolean` | 必须提供默认值；生成 `--flag` / `--no-flag` |
| `kat.Duration` | `duration` | 非负十进制加 `ns/us/ms/s/min/h` |
| `kat.WallClockTimestamp` | `wall_clock_timestamp` | RFC 3339，必须有 `Z` 或已知显式 UTC offset |
| `Literal["a", "b"]` | `string` choices | 只能包含 string，大小写敏感 |
| 非 bool 的 `T \| None` | 对应的可选类型 | 必须默认 `None` |

非 bool 参数没有默认值时是 required；有合法默认值时是 optional。不要使用 `Any`、容器、任意
Union、Path 或自定义 class 作为 Workflow 参数。秘密也不要作为 Workflow 参数传递，因为
`kat run` 的参数会保留在 Operation log，并进入 Run Manifest 的有效输入。环境变量本身不会作为
Workflow 参数投影，但这不等于 KAT 会自动脱敏：Runtime 的 stdout/stderr、Python logging 和异常链
文本也会进入诊断或 Operation log。不要打印环境、连接串、凭据或敏感结果，也不要把秘密拼进异常
消息；外部凭据场景应使用合成 sentinel 做日志泄漏检查。

依据：`kat/platform/workflow/api/_workflow.py:107-129`、
`kat/platform/workflow/runtime/inspection.py:231-321`、
`kat/platform/cli/src/lib.rs:56-70`、`kat/platform/cli/src/run.rs:63-76,129-139`、
`kat/platform/cli/src/workflow_runtime.rs:532-561`、
`kat/platform/workflow/runtime/diagnostic.py:25-56`。

### 6.3 `required_tables` 与 Dataset

`required_tables` 是 Workflow SQL 的最小 Dataset 表授权，不是文档提示：

- 使用 `required_tables=["events", "process"]` 时，生产 `kat run` 必须传一个合法 Dataset，且
  该 Dataset 必须同时包含这两张表。
- Runtime 只把已声明的表以裸表名注册进 Workflow 的 DataFusion session；SQL 写
  `FROM events`，不是 `FROM dataset.events`。
- 没有声明的 Dataset 表不会进入该 Workflow SQL session；当前没有 `ctx.table()` 旁路。
- 使用 `required_tables=[]` 时可以不传 Dataset。即使传了 Dataset，Workflow SQL 也不会自动
  获得其中全部表。
- 表名必须使用小写 ASCII snake_case，并排除 Windows 设备名。重复声明会被去重、排序。

Dataset 型 Workflow 示例：

```python
import kat


@kat.workflow(
    name="count-events",
    title="Count Events",
    required_tables=["events"],
    parameters={},
)
def count_events(ctx: kat.Context):
    """Count the currently granted Dataset events by event type."""
    return {
        "event_counts": ctx.sql(
            """
            SELECT event_type, COUNT(*) AS event_count
            FROM events
            GROUP BY event_type
            ORDER BY event_count DESC, event_type ASC
            """
        )
    }
```

不要复制示例里的 `events` 契约到真实 PACK；先用 `kat inspect --dataset` 读取目标 Dataset 的
实际表名、列名、Arrow 类型与 nullability，再声明最小需要的表和 SQL。

依据：`kat/platform/workflow/api/_workflow.py:16-22,181-186`、
`kat/platform/workflow/runtime/execution.py:126-147`、
`docs/adr/0020-dataset-writes-hide-physical-layout.md:9-13`。

### 6.4 Context 的三个能力

#### `ctx.sql(sql, **params) -> DataFrame`

- 接受一条非空的只读 DataFusion SQL statement。
- 禁止 DDL、DML、COPY、session mutation 和 multiple statements。
- `$name` 只绑定同名 keyword value，不做字符串替换，也不能绑定标识符或 SQL 片段。
- 参数值仅接受 bool、int64、有限 float、str、`kat.Duration`、
  `kat.WallClockTimestamp`。
- DataFrame 是 lazy 的，必须在 Workflow 返回值中交给 KAT 物化。

例如：

```python
frame = ctx.sql(
    "SELECT * FROM events WHERE category = $category",
    category=category,
)
```

#### `ctx.from_arrow(table) -> DataFrame`

只接受 `pyarrow.Table`，用于把 Python 或 Host 客户端库取得的结构化结果放入当前 DataFusion
execution plane。RecordBatch、list、Pandas DataFrame 或其他 table-like object 不属于合同；先显式
转换为 `pyarrow.Table`。

#### `ctx.convert_clock(clock_domain, clock_value, *, target_domain) -> Expr`

这是特定的时钟换算能力，输入是 DataFusion Expr。只有确实使用 KAT Dataset 时钟事实的 PACK
才需要它；使用该能力的 Run 必须传 Dataset，即使 `required_tables=[]`。Runtime 通过私有
ClockCapability 读取 `clock_domain`、`clock_snapshot` 等时钟证据，它们不因这项能力而加入
Workflow SQL 的 `required_tables`。该方法不是通用时间解析器，也没有同名 SQL 函数。

Context 没有 `ctx.table`、`ctx.output`、`ctx.log`、PACK discovery、Dataset path、底层
`SessionContext` 或依赖查找 API。使用标准库 `logging` 写诊断；需要 Context 的 helper 应显式
接收它。

依据：`kat/platform/workflow/api/_workflow.py:16-81`、
`kat/platform/workflow/runtime/execution.py:51-93,172-187`、
`docs/adr/0033-workflows-derive-data-without-mutating-datasets.md:5-7`。

### 6.5 Workflow 返回值与 Output

合法返回值只有：

1. 一个 DataFusion `DataFrame`，KAT 将它命名为 `main`；或
2. 一个精确、非空的 `dict[str, DataFrame]`，显式声明一个或多个 Output。

推荐始终返回领域名称明确的单元素或多元素 dict。不能直接返回 PyArrow Table、list、tuple、
generator、scalar、`None` 或空 dict。若先得到 PyArrow Table，返回
`{"name": ctx.from_arrow(table)}`。

Output name 必须是小写 ASCII snake_case，并排除 Windows 设备名。它会原样成为：

- `kat run` 响应中 `result.outputs` 的 key；
- Run Manifest 的 Output identity；
- `kat query` 中 `output.<name>` 的表名。

KAT 不在装饰器中声明 Output schema。实际 DataFrame schema 是唯一执行事实；零行结果合法，但
必须仍有确定 schema。多 Output 在逻辑上 all-or-fail，任何一个物化失败都不会发布成功 Run。

依据：`kat/platform/workflow/api/_workflow.py:131-138`、
`kat/platform/workflow/runtime/outputs.py:20-69`、
`docs/adr/0055-run-publication-requires-portable-output-names.md:11-29`。

## 7. Helper 与依赖边界

### 7.1 PACK 内代码复用

把普通共享实现放在 `helpers/`，例如：

```python
from kat.pack.helpers.rules import normalize
```

不要从一个 `kat.pack.workflows.*` 入口 import 另一个入口；不要按物理目录名 import PACK；不要
跨 PACK import。第一版每个 PACK 是完全自包含的执行单位，也没有 manifest dependency 或 exported
capability。

Workflow 和 helper 的 module global 只放 import、不可变常量、无状态定义与注册，不保存一次
Workflow/Test 执行的可变状态。

依据：`docs/adr/0027-first-version-packs-are-self-contained.md:5-9`、
`docs/adr/0047-current-pack-is-exposed-as-kat-pack.md:7-17`、
`docs/adr/0016-pack-inspect-and-test-separate-production-and-test-constraints.md:31-35`。

### 7.2 Platform 公共 common

稳定、被多个真实 PACK 复用的代码能力可以随 KAT Platform Host 发布在 `kat.common`。它与
`kat` Authoring API、私有 Runtime 位于同一个 wheel/Skill 交付中，不是一个独立安装、独立版本
或可跨 PACK import 的“common PACK”。普通新能力应先在所属 PACK 的 `helpers/` 中验证；只有
公共语义已经稳定时才进入 Platform common。

首版 PostgreSQL 接口为：

```python
from pathlib import Path

from kat.common.sql import postgresql

sql_file = (Path(__file__).resolve().parents[1] / "queries" / "summary.sql").resolve()
frame = postgresql.execute_sql_file(
    ctx,
    sql_file_path=sql_file,
    parameters={"day": day},
)

generated = postgresql.execute_sql_text(
    ctx,
    sql_text="SELECT * FROM events WHERE event_day = %(day)s",
    parameters={"day": day},
)
```

两者直接返回可作为 Workflow Output 的 DataFusion DataFrame。文件接口要求绝对路径并以
`utf-8-sig` 严格读取；common 不扫描资源目录、不使用当前工作目录、不展开环境变量、`~` 或
通配符。多个 PACK 需要共享固定 SQL 时，可以约定同一个外部绝对路径，但该路径因此成为各
PACK 的部署合同。若 SQL 随 PACK 交付，应像示例一样根据模块 `__file__` 定位。

PostgreSQL 连接由 Psycopg/libpq 从进程环境解析；常用合同是 `PGHOST`、可选
`PGHOSTADDR`、`PGPORT`、`PGDATABASE`、`PGUSER`、`PGPASSWORD`、`PGSSLMODE`、可选
`PGSSLROOTCERT`、`PGCONNECT_TIMEOUT` 和 `PGCLIENTENCODING`。不要把凭据加入 Workflow
参数；需要排除机器上其他 `PG*` 配置时，应像离线开发包脚本一样先清理再设置白名单。每次
调用建立并关闭一个
`autocommit=True` 短连接；SQL 使用 Psycopg `%(name)s` 参数绑定，不能用字符串替换。一个
调用必须恰好返回一个列名非空且唯一的 rowset；结果按封闭类型集合转换为 Arrow，未知类型要求
SQL 显式 `CAST`。

完整示例、类型边界和受限网络开发包见
[`postgresql-pack-development.md`](postgresql-pack-development.md)。

### 7.3 Python 第三方依赖

External PACK 只分发源码和领域资源，不拥有依赖安装阶段：

- `pack.toml` 不能声明 dependencies。
- PACK 内增加 `requirements.txt`、`pyproject.toml` 或调用 pip，不会让 KAT 自动安装依赖。
- 系统 Python、用户 site-packages 与 `PYTHONPATH` 不参与受支持执行。
- 只能 import 目标 KAT Skill 的 Bundled Python Host 已经携带的包。
- 当前 Workflow Host 明确携带 Click、DataFusion、PyArrow、pytest、Psycopg、openpyxl、
  XlsxWriter、defusedxml 以及 `kat.common`；精确版本见实际 Release/Host 验证结果。这些锁定
  版本不构成跨 KAT 版本承诺。

如果真实 PACK 必须使用新的第三方库，先把“目标平台 Host 已携带并验证该库”作为独立交付前提；
否则这不是一个只改 External PACK 就能完成的任务。不要因为系统 Python 可以 import 就宣布完成。

openpyxl 可读取、修改和写入 `.xlsx/.xlsm`，XlsxWriter 只生成 `.xlsx`；PACK 直接 import，
KAT 首版不提供 `kat.common.excel` 包装。旧 `.xls`、`.xlsb` 和 pandas 未预装。PostgreSQL 与
Excel 能力都必须在实际 Bundled Host 中验证，系统 Python 可导入不构成交付证据。

依据：`docs/adr/0003-external-packs-are-a-first-class-deployment-unit.md:7-11`、
`docs/adr/0004-supported-execution-requires-the-bundled-python-host.md:7-13`、
`kat/platform/workflow/pyproject.toml:1-13`。

## 8. Dataset 与 Test Dataset

### 8.1 先检查生产 Dataset

Dataset 型 PACK 开发前执行：

```powershell
$Dataset = (Resolve-Path "C:\path\to\dataset").Path
$inspection = & $Kat inspect --dataset $Dataset | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $inspection.status -ne "success") {
    throw "Dataset inspection failed"
}
$inspection.result | ConvertTo-Json -Depth 20
```

只根据成功响应中的 `result.path`、`result.tables[].name` 和 columns 设计 Workflow。不要扫描
Dataset 私有布局或猜表名。

### 8.2 建立 Test Dataset

需要生产执行面集成测试时，把一个完整普通 KAT Dataset 放在：

```text
tests/datasets/<selector>/
```

推荐使用对应内置 Datasource 的正常 import 创建或整体修订：

```powershell
& $Kat import --help
```

先从帮助中确认目标 KAT 版本确实提供能读取该 Source 的内置 Datasource；如果没有，应停止并重新
设计数据路径，External PACK 不能补出新的 `kat import` 变体。

首次创建 Test Dataset：

```text
kat import --dataset <pack>/tests/datasets/sample <datasource-type> <typed arguments>
```

整体修订一个已经非空的 Test Dataset 时，必须显式授权覆盖：

```text
kat import --dataset <pack>/tests/datasets/sample --overwrite-dataset <datasource-type> <typed arguments>
```

`--overwrite-dataset` 会永久清空解析后的整个目标目录，包括未识别文件，不提供备份、回滚或失败
恢复。执行前必须确认 Source 不在目标目录内，并确认目标路径就是要整体替换的 Test Dataset。

具体 `<datasource-type>` 和参数以该版 `kat import --help` 为准。Test Dataset 应与 PACK 一起版本化。
测试用 `kat_run(workflow="...", dataset="sample", arguments=[...])` 选择它；`sample` 是一级
目录名，不是绝对或相对路径。

`tests/datasets/` 缺失或没有 candidate 可以表示零 Test Dataset，不影响无 Dataset 测试。手工
拼 `.kat-dataset` 或 Parquet 文件树会绕过 Dataset Storage 的写入合同，不应成为标准流程。

依据：`docs/adr/0016-pack-inspect-and-test-separate-production-and-test-constraints.md:13-23`、
`kat/platform/cli/src/test.rs:190-249`、
`kat/platform/workflow/runtime/testing.py:65-113`。

## 9. 必须完成的开发闭环

完成第 4、5 节的三个示例文件后，再解析 PACK 根目录：

```powershell
$Pack = (Resolve-Path ".\example-values").Path
```

### 9.1 Inspect：验证生产 Interface

```powershell
$inspect = & $Kat inspect `
    --pack example-values `
    --pack-dir $Pack | ConvertFrom-Json

if ($LASTEXITCODE -ne 0 -or $inspect.status -ne "success") {
    $inspect | ConvertTo-Json -Depth 20
    throw "PACK inspection failed"
}
$inspect.result | ConvertTo-Json -Depth 20
```

检查成功结果中的：

- PACK `name/title/description/owner`；
- `workflows[].name/title/description`；
- `required_tables`；
- 每个参数的 option、type、required/default、choices 与说明。

`inspect` 会导入并校验生产 Interface，但不会执行 Workflow SQL，也不会证明真实 Dataset 或外部
系统可用。

### 9.2 Test：通过 KAT 执行 pytest

```powershell
$test = & $Kat test --pack-dir $Pack | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $test.status -ne "success") {
    $test | ConvertTo-Json -Depth 20
    throw "PACK tests failed"
}
$test | ConvertTo-Json -Depth 20
```

精确重跑一个失败测试：

```powershell
& $Kat test `
    --pack-dir $Pack `
    --test "tests/test_build_summary.py::test_build_summary_runs_through_production_interface"
```

只把成功 KAT Response 的 `result.summary` 当作测试结论。失败时同时保留 `error`、可用的
`log_path` 和 `test_report_path`。pytest terminal report 用于定位，不替代 KAT Response 状态。

`tests/` 不存在、没有收集到测试、collection error 或用例失败都会使 `kat test` 失败；纯运行
部署可以省略测试，但不能再声称 `kat test` 已验证。

### 9.3 Run：发布一次真实 Run

无 Dataset 模板：

```powershell
$run = & $Kat run `
    --pack example-values `
    --workflow build-summary `
    --pack-dir $Pack `
    -- `
    --label "demo" `
    --item-count 2 | ConvertFrom-Json

if ($LASTEXITCODE -ne 0 -or $run.status -ne "success") {
    $run | ConvertTo-Json -Depth 20
    throw "Workflow run failed"
}
$run.result | ConvertTo-Json -Depth 20
```

Dataset 型 Workflow 在 `--` 之前增加：

```text
--dataset <Dataset目录>
```

Workflow 业务参数必须放在单独的 `--` 之后，并以 inspection 返回的 option 为准。成功结果中保存：

```powershell
$RunId = $run.result.run_id
$OutputNames = @($run.result.outputs.PSObject.Properties.Name)
```

不要只记 Run ID 而丢弃 Output name 和 columns；后续 Query 需要这些执行事实。

`--` 后的路径值不由 KAT CLI 解释。生产 `kat run` 通常继承调用进程的当前目录，而 `kat test`
会把 Host 工作目录固定为 PACK 根，因此同一相对路径可能测试通过、生产却读取另一个位置。路径输入
优先传已经解析的绝对路径；若 Workflow 有意接受相对路径，必须明确其基准并分别测试生产与测试
形态。PACK 自带资源应根据当前 Workflow/helper 模块的 `__file__` 定位，不要依赖当前工作目录。

### 9.4 Query：只读查询 Run Output

```powershell
$query = & $Kat query `
    --run $RunId `
    --sql "SELECT label, item_count FROM output.summary LIMIT 10" |
    ConvertFrom-Json

if ($LASTEXITCODE -ne 0 -or $query.status -ne "success") {
    $query | ConvertTo-Json -Depth 20
    throw "Run query failed"
}
$query.result | ConvertTo-Json -Depth 20
```

成功 `result` 的固定结构是：

```json
{
  "dataset": {"status": "not_provided"},
  "columns": [
    {"name": "label", "type": "string"},
    {"name": "item_count", "type": "int64"}
  ],
  "rows": [["demo", "2"]]
}
```

`rows` 是 positional arrays，值按位置对应 `columns`；Arrow Int64/UInt64 和 Decimal 使用十进制
JSON string，较小整数和有限 float 使用 JSON number，null 保持 JSON null。Run 引用了 Dataset 时，
Query 还可以访问 `dataset.<table>`，但它读取的是该路径当前可用的 Dataset，不是 Run 时快照。

当前实现不会自动增加 `LIMIT`，也没有分页、流式结果、固定 query timeout 或旧版 ADR 曾描述的
行数/字节上限；Runtime 会 collect 完整结果。开发闭环和 AI 调用应主动做投影、过滤、聚合并给明细
SQL 加 `LIMIT`，避免把大型结果送进 stdout 或模型上下文。

虽然 SQL 是只读的，但 Query 当前允许 DataFusion URL/local table，并非只准访问两个已注册 schema。
它把调用方和 SQL 当作受信任本地输入，不提供来源 allowlist 或不可信 SQL 沙箱。

依据：`kat/skill/references/command-reference.md:27-66`、
`kat/platform/workflow/runtime/query.py:32-75,78-116`、
`kat/platform/workflow/tests/test_query_process.py:204-228`、
`docs/adr/0038-query-results-use-compact-positional-json.md:5-11`、
`docs/adr/0056-cli-runtime-and-bundled-python-host-use-trusted-local-ipc.md:76-85`。

## 10. 测试策略

一个可交付 PACK 至少覆盖以下层次：

1. **生产 Interface**：`kat inspect --pack` 成功，并人工核对 Workflow、Required tables 和参数投影。
2. **生产执行 seam**：至少一个 pytest 用例通过 `kat_run` 执行每个 Workflow，不只直接调用 Python
   函数或 Fake Context。
3. **领域语义**：覆盖最小正常输入、边界值、排序/聚合规则、null 规则和确定性输出。
4. **空结果**：若业务允许零行，断言零行时仍保留预期 schema。
5. **Dataset 边界**：Dataset 型 PACK 用版本化 Test Dataset 验证真实 `required_tables` 和实际
   DataFusion/Parquet 读取。
6. **完整闭环**：至少执行一次生产 `kat run`，再根据成功响应中的真实 Output 名和 columns 执行
   `kat query`。
7. **外部依赖**：依赖数据库、网络服务或本地 native 库时，额外验证目标 Host 中的依赖、成功路径、
   失败诊断和秘密不泄漏；不要用系统 Python 的结果替代。

纯 helper 可以增加普通单元测试，但它们不能替代 `kat_run`。测试应断言表内容和 schema，而不是只
断言“没有抛异常”。

依据：`docs/adr/0016-pack-inspect-and-test-separate-production-and-test-constraints.md:9-39`、
`kat/platform/workflow/runtime/testing.py:61-115`。

## 11. 常见错误与修复方向

| 现象 | 常见原因 | 修复方向 |
| --- | --- | --- |
| `KAT Skill is unavailable` 或 Host 启动失败 | 使用了 Cargo 裸 CLI，或 Skill/Payload 目录不完整 | 改用完整 Skill 中与 Python Host 相邻的 `kat`/`kat.exe` |
| PACK 找不到 | `--pack-dir` 指到父目录；manifest `name` 与 `--pack` 不一致 | 让 `--pack-dir` 精确指向含 `pack.toml` 的目录，并使用 manifest name |
| manifest parse/discovery 失败 | 多了 `version`/`dependencies`/`[pack]`，字段缺失或重名 | 保留恰好四个根级 string，并消除 PACK name 冲突 |
| inspection 报入口注册错误 | 一个文件零个/多个 Workflow；import 了另一入口；helper 副作用注册 | 每个入口恰好一个本 module Workflow，共享实现移到无注册副作用的 helper |
| inspection 报签名错误 | 缺 docstring、首参不是 `ctx: kat.Context`、参数说明不匹配、类型不支持 | 按 inspection 的封闭类型合同逐项修正 |
| Workflow requires a Dataset | `required_tables` 非空但 Run/Test 未选择 Dataset | 生产传 `--dataset`；测试传 Test Dataset selector |
| Dataset missing required tables | 表名写错或 Test Dataset 与生产契约不一致 | 先 `kat inspect --dataset`，修订最小 `required_tables` 或重建 Test Dataset |
| SQL 找不到 Dataset 表 | 在 Workflow 中写了 `dataset.events`，或没有声明表 | Workflow `ctx.sql` 使用已授权的裸表名 `events`；`dataset.*` 只用于 Run Query |
| 返回值类型错误 | 直接返回 PyArrow Table、None、list 或空 dict | 用 `ctx.from_arrow(table)`，返回 DataFrame 或非空具名 DataFrame dict |
| Output name 非法 | 使用 kebab-case、大写、空格或 Windows 设备名 | 改成合法且有业务含义的 snake_case |
| `ModuleNotFoundError` 只发生在 KAT | 依赖只装在系统 Python；跨 PACK import；目标 Skill 版本尚无所需 `kat.common` | 只使用实际 Bundled Host 已验证的依赖、`kat.common.*` 和 `kat.pack.helpers.*`；否则先改 Host 交付 |
| PostgreSQL SQL 文件被拒绝 | 向 `execute_sql_file()` 传了相对路径、环境变量占位或通配符 | 在 Workflow 中明确构造并传入绝对路径 |
| PostgreSQL→Arrow 类型失败 | 返回数组、JSON、UUID、枚举、复合/扩展类型或无确定精度 numeric | 在远程 SQL 中显式 `CAST` 为 common 支持的标量类型 |
| `kat test` 没有测试 | `tests/` 缺失或 pytest 零收集 | 添加可收集的 `tests/test_*.py`，以 `kat_run` 覆盖生产 seam |
| `kat query` 类型失败 | Output/SQL 暴露了尚不支持的 Arrow complex scalar 或非 UTC ns timestamp | 在 Workflow 或 Query SQL 中显式投影为支持的 scalar 类型 |
| Query 输出过大或很慢 | 假设 KAT 会自动 LIMIT、分页或 timeout | 主动投影、过滤、聚合，并为明细查询写显式 LIMIT |
| 凭据进入日志或 Manifest | 把秘密放进 Workflow arguments/Query SQL，或由 print、logging、异常消息回显 | 改用目标库支持的环境变量/外部凭据机制，禁止回显，并验证 Response、stderr 和日志不含 sentinel |

## 12. 当前明确不保证的能力

写设计或交付说明时，不要暗示 KAT 已提供下列能力：

- External PACK 动态注册 Datasource 或扩展 `kat import`；
- PACK manifest dependency、跨 PACK import、公共 common PACK；
- PACK 自动安装 Python dependency，或回退到系统 Python/venv；
- `ctx.table()`、`ctx.output()`、`ctx.log()`、底层 SessionContext 或 Dataset path 访问；
- Workflow 修改、扩展或写回 Dataset；
- `ctx.sql()` 执行 DDL、DML、COPY 或 session mutation；PostgreSQL common 会把任意 SQL 原样交给
  数据库，权限边界由数据库账号承担，KAT 不为它提供 SQL 沙箱；
- 静态 Output schema/description 声明，或返回 PyArrow Table 自动转换；
- Run Output 分页、流式 stdout、自动 LIMIT、固定 query timeout 或静默截断；
- Query 来源隔离、来源 allowlist 或不可信 SQL 沙箱；当前 URL/local table 没有被统一封锁；
- 受信任 PACK 的安全沙箱；PACK 与测试代码可以按普通本地代码产生副作用；
- daemon、REST server 或长期 Runtime 进程；
- 失败 Output 的事务回滚、自动清理、崩溃恢复或历史迁移；
- PACK Authoring Interface、私有 Runtime IPC、Python package 版本的跨 KAT 版本兼容。

依据：`docs/adr/0004-supported-execution-requires-the-bundled-python-host.md:7-13`、
`docs/adr/0022-datasource-types-are-closed-and-bundled.md:7-9`、
`docs/adr/0027-first-version-packs-are-self-contained.md:7-9`、
`docs/adr/0032-workflow-execution-capabilities-require-explicit-context.md:13-23`、
`docs/adr/0056-cli-runtime-and-bundled-python-host-use-trusted-local-ipc.md:26-35,76-85`。

## 13. 给执行 AI 的交付清单

在宣布一个新 PACK “开发完成”前，逐项给出证据：

- [ ] 已确认目标仓库的协作协议、issue/轻量 SDD 和本次非目标。
- [ ] 已确认完整 KAT Skill 路径和目标 KAT 版本，不使用 Cargo 裸 CLI 或系统 Python 冒充 Host。
- [ ] `pack.toml` 只有四个合法字段，PACK name 与命令一致。
- [ ] 每个 `workflows/*.py` 恰好一个 Workflow，helper 只放在 `helpers/`。
- [ ] `required_tables` 与实际 Dataset inspection 一致；无 Dataset 能力明确使用空列表。
- [ ] 参数类型、默认值、说明和 CLI option 经过成功 inspection 核对。
- [ ] 返回值是 DataFusion DataFrame 或非空具名映射，Output name 合法且语义明确。
- [ ] 所有第三方 import 在实际 Bundled Host 中验证，不依赖系统 Python 的偶然状态。
- [ ] 使用 PostgreSQL common 时，SQL 文件路径为绝对路径、值使用 named binding、凭据只在
  `PG*` 进程环境中，并覆盖零行、NULL、类型失败和恰好一个 rowset 的边界。
- [ ] `kat inspect --pack` 成功并保存成功 Response。
- [ ] `kat test` 成功；报告实际 summary，并保留 log/report 路径作为证据。
- [ ] 生产形态的 `kat run` 成功，保存真实 `run_id`、Output names、columns 和 row counts。
- [ ] 使用真实 Output name 执行有界 `kat query`，核对 columns 与 positional rows。
- [ ] 交付变更文件、实际命令/结果、未覆盖环境与仍存限制；没有把失败或未运行包装成成功。

## 14. 一方事实索引

若本文与实现发生冲突，以目标 KAT 版本的源码、测试和成功 KAT Response 为准。重点入口：

- PACK discovery 与 manifest：`kat/platform/cli/src/pack_discovery.rs`
- CLI 命令形状：`kat/platform/cli/src/lib.rs`、`run.rs`、`query.rs`、`test.rs`
- Pack Authoring API：`kat/platform/workflow/api/_workflow.py`
- Platform common PostgreSQL：`kat/platform/workflow/common/sql/postgresql.py`
- Workflow 参数编译：`kat/platform/workflow/runtime/inspection.py`
- PACK 源码扫描/加载：`kat/platform/workflow/runtime/pack.py`
- Workflow execution plane：`kat/platform/workflow/runtime/execution.py`
- Output 物化：`kat/platform/workflow/runtime/outputs.py`
- `kat_run` pytest fixture：`kat/platform/workflow/runtime/testing.py`
- Run Query：`kat/platform/workflow/runtime/query.py`
- 作者 API 合同测试：`kat/platform/workflow/tests/test_authoring_api.py`
- Query 当前无旧限制的回归测试：`kat/platform/workflow/tests/test_query_process.py:204-228`
- External PACK / Host / layout / Context 决策：`docs/adr/0003`、`0004`、`0017`、`0032`、`0047`
- 命令速查：`kat/skill/references/command-reference.md`
- 已交付的外部数据源案例：`examples/packs/postgresql-query/`
