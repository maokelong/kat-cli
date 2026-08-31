# PACK 创作与维护流程

## 1. 定位并先检查 PACK

用户指定已有 PACK 时，先通过私有 `kat inspect --pack` 和需要时的精确 `--pack-dir` 定位它。只在成功 Response 中读取 PACK identity、Workflow、参数和验证入口；inspection 失败时停止，保留 Diagnostic 和可用 `log_path`，按 [result-contract.md](result-contract.md) 交付。不要自行扫描 manifest、猜测 PACK 目录或加载 PACK Python 代码来替代 inspection。

新建 PACK 没有 KAT 任务契约内的 Issue 或 SDD 前置门；执行所在仓库的协作规范仍独立适用。

## 2. 使用当前作者合同

Workflow 是普通模块顶层同步函数，由 `@workflow(name=..., title=..., parameters=...)` 声明。Runtime 以 `ctx: Context` 和解析后的具名输入显式调用选中的函数。Context 只提供 `ctx.datasource_root`；PACK 不从 Context 取得来源查询、Arrow 转换、时钟转换或隐式 relation catalog。

PACK 在顶层 `datasources/` 中拥有普通 Python 模块和 Provider 类。Workflow 像调用其他 PACK 代码一样显式构造并调用它们；KAT 不扫描、注册、构造或包装 Provider。文件 Provider 应在 `ctx.datasource_root` 下建立当前 Workflow 的临时 workspace，向 Provider 传普通路径，并在 eager Table 脱离来源后清理临时物化。

`kat-workflow` 与 `kat-datasource` 是 Payload 中两个独立的私有 wheel：

- `kat-workflow` 提供 `kat.workflow`、`kat.Context`、`kat.Duration`、`kat.WallClockTimestamp`、`kat.dataprovider` 和私有 Runtime；
- 平台原生 `kat-datasource` 提供窄的 `kat_datasource` 来源 API；它不依赖或重新导出 `kat`。

两个 wheel 随同一 KAT 版本原子安装，但 PACK 必须分别显式 import 所需模块，不能假设一个 distribution 会传递另一个。

标准 Output 只能是精确的 `dp.Table`，或一个非空普通 `dict`，其中每个键都是有效 Output 名称、每个值都是精确 `dp.Table`。PyArrow Table、引擎惰性值、Table/dict 子类、空 Mapping 和混合值都不是 Output。

## 3. 显式来源解码与融合

原生 Hitrace 解码由 PACK 显式调用：

```python
from pathlib import Path
from tempfile import TemporaryDirectory

from kat import Context, dataprovider as dp, workflow
from kat_datasource import hitrace


@workflow(
    name="summarize-trace",
    title="Summarize trace",
    parameters={"source_path": "Hitrace source path."},
)
def summarize_trace(ctx: Context, *, source_path: str):
    """Decode and summarize one Hitrace source."""
    with TemporaryDirectory(dir=ctx.datasource_root) as temporary:
        relations = Path(temporary) / "relations"
        report = hitrace.decode(Path(source_path), relations)
        catalog = dp.open(root=relations)
        return dp.DataFusionProvider(catalog=catalog).query("SELECT ...")
```

`hitrace.decode()` 要求 destination 尚不存在；成功后 destination 的直接子级只含扁平具名 Parquet relation，并返回不可变 `DecodeReport`，列出 unsupported plugin 和 section type。它不创建平台来源身份或持久状态。失败时不要把残留路径、部分 relation 或 unsupported report 当作成功。

`dp.open(root=...)` 发现一个 flat Parquet 目录；`dp.open(tables=...)` 绑定明确的 relation 路径。需要跨来源融合时，Workflow 先显式调用每个 Datasource Provider 得到 eager Table 或 Catalog，再把具名内存 Table 和一个磁盘 Catalog 交给普通 DataFusion Provider：

```python
local = dp.open(tables={"placement": placement_path})
result = dp.DataFusionProvider(
    tables={"telemetry": telemetry_provider.query(...)},
    catalog=local,
).query("SELECT ...")
```

DataFusion Provider 只看构造时显式传入的 relation，不发现来源 Provider、不触发远端查询，也没有跨 Workflow Session。

## 4. 只读理解、校验和测试

对于“理解 PACK”，交付它解决的问题与 Workflow、每个 Workflow 的显式来源或参数、已有测试或验证证据、明确限制与下一步；不要复述目录、manifest 或源码。

对于校验或测试，先 inspection，再执行私有 `kat test`。测试用普通 fixture 构造来源文件、Provider 配置和临时路径；`kat_run` 只选择 Workflow 并传 arguments。只从 success Response 的 `summary`，以及存在时的 `test_report_path` 与 `log_path` 判断和引用测试证据。test failure 时停止成功路径，保留 failure Response 的 Diagnostic、可用报告与日志，按 [result-contract.md](result-contract.md) 交付。

## 5. 已授权的变更或修复

只有用户明确要求创建、修改或修复时才能写入指定 PACK 源码。变更前先确认目标 PACK 位置与用户目标；保持最小切片，不修改 Skill、Platform Payload 或无关 PACK。

每次写入后：

1. 重新 inspection，确认生产 Interface、显式参数和 Output 合同。
2. 运行适用的 `kat test`；失败时使用报告和日志诊断，但不把失败说成完成。
3. 交付变更摘要、受影响文件、实际验证证据和仍存限制。

“诊断失败”本身不授权修复。无法在现有授权和事实下继续时，按 [result-contract.md](result-contract.md) 交付最小下一步。
