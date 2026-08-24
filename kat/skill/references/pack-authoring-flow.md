# PACK 创作与维护流程

## 1. 定位并先检查 PACK

用户指定已有 PACK 时，先通过私有 `kat inspect --pack` 和需要时的精确 `--pack-dir` 定位它。只在成功 Response 中读取 PACK identity、`source_guide`、Sources、Workflows、参数和验证入口；inspection 失败时停止，保留 Diagnostic 和可用 `log_path`，按 [result-contract.md](result-contract.md) 交付。不要自行扫描 manifest、猜测 PACK 目录或加载 PACK Python 代码来替代 inspection。

KAT 自身不额外设置 Issue 或 SDD 前置门；仍须遵守所在仓库的协作规范。

## 2. 只读理解、校验和测试

对于“理解 PACK”，交付它解决的问题、Source tables 及字段语义、可复用 Analysis、Workflows、参数、已有测试或验证证据、明确限制与下一步；不要复述目录、manifest 或源码。

对于校验或测试，先 inspection，再执行私有 `kat test`。只从 success Response 的 `summary`，以及存在时的 `test_report_path` 与 `log_path` 判断和引用测试证据。test failure 时停止成功路径，保留 failure Response 的 Diagnostic、可用 `test_report_path` 与 `log_path`，按 [result-contract.md](result-contract.md) 交付。pytest terminal report 用于定位失败 node ID 和解释断言，但不推断 KAT 操作状态。

## 3. 已授权的变更或修复

只有用户明确要求创建、修改或修复时才能写入指定 PACK 源码。变更前先确认目标 PACK 位置与用户目标；保持最小切片，不修改 Skill、Platform Payload 或无关 PACK。来源扩展沿用以下边界：

- `SOURCES.md` 说明 Source tables、字段语义、输入形态和限制；
- `sources/` 中每个入口文件定义一个 `@kat.source(name=...)`，返回一个自行定义表集合、可由 DataFusion 注册的 schema-provider 值；Source 及其 helper 用 `from ..decoders.example import ...` 这类包内相对导入；
- `analysis/` 保存直接查询 `<source>.<table>` 的可复用关系或算法；
- `workflows/` 只解释任务输入、调用 Analysis 并发布 Run Outputs，不重复声明表依赖。

Python 解析器优先通过 `kat.schema_from_readers` 提供按表延迟创建的 `pyarrow.RecordBatchReader`；已有 DataFusion/PyArrow Provider 则直接复用。KAT 的 Source inspection、Binding、Materialization 和 External resolution 统一使用按 PACK 隔离的 Runtime 私有 module root；公开 `kat.pack` 只表示当前 Workflow PACK，供 Workflow、Analysis 与测试使用。Source Entry 的参数由 `kat inspect --pack` 投影，用户说明只写在 Source Guide。生产配置通过 Dataset Binding 取得，不把 Source 参数混入 Workflow 参数。

KAT 会按普通 Python 语义导入每个被扫描的 Source 入口，以取得 decorator 注册结果；不会读取或分析源码文本。入口模块顶层只做声明和轻量 import，不在 import 时连接数据库、读取数据文件、创建 Provider 或产生外部副作用。只由某个 Source 使用的可选 Python/native 依赖放在该 Source Entry 或其延迟 Provider factory 内，使缺失依赖只在实际使用该 Source 时失败。已有 Binding 的 Query/Run resolution 使用相同的私有 module namespace，但不读取 `SOURCES.md`；Guide 是选择、inspection、test、bind 与 materialize 的作者合同，不是已绑定查询的运行依赖。

### 当前 Bundled Python Host

本版本 Payload 实际锁定的 Host 能力如下；只有标为“生产作者 Interface”的项目可由 Source/Analysis/Workflow 直接依赖。PACK 不运行时安装依赖，也不假设开发机 `site-packages` 会进入 Payload。

| 范围 | 能力 | 锁定版本 |
| --- | --- | --- |
| 生产作者 Interface | CPython | `3.14.6` |
| 生产作者 Interface | KAT Python API | 与当前 Payload 同版本 |
| 生产作者 Interface | DataFusion Python | `54.0.0` |
| 生产作者 Interface | PyArrow | `24.0.0` |
| PACK 测试 | pytest | `9.1.1` |
| 平台私有实现，PACK 不得依赖 | Click | `8.4.2` |

PostgreSQL、Excel 等专用 driver 当前不在 Host 中。“复用既有设施”在本版本是指：直接返回 Host 已能注册的 DataFusion/PyArrow Provider，读取已支持的本地或远端 Parquet，或接入外部工具已经提供的 Parquet/Flight SQL/官方 DataFusion FFI 边界；不表示 PACK 可以任意 import 未随 Payload 交付的库。只有形成真实跨 PACK 需求并完成 Windows/Linux wheel 验证后，平台才增加锁定依赖。

本地 Parquet 的最小 Source 可以直接复用公开 Provider：

```python
from pathlib import Path

from datafusion.catalog import Schema, Table
import pyarrow.dataset as ds
from kat import source


@source(name="logs")
def logs(path: Path):
    schema = Schema.memory_schema()
    schema.register_table("events", Table(ds.dataset(path, format="parquet")))
    return schema
```

普通 Python Parser 则保持表级延迟创建 reader：

```python
from pathlib import Path

import kat
import pyarrow as pa


@kat.source(name="logs")
def logs(path: Path):
    def events():
        stream = path.open("rb")
        # Parser 负责从 stream 增量构造与 schema 一致的 RecordBatch。
        return pa.RecordBatchReader.from_batches(schema, batches(stream))

    return kat.schema_from_readers({"events": events})
```

示例中的 `schema`、`batches` 与输入关闭策略属于 PACK Parser；KAT 只适配标准 `RecordBatchReader`。远端 Parquet 使用同一 Provider 形态并显式传入所选设施支持的 filesystem/凭据；PG、Excel 等当前没有 Bundled driver 时，先由既有设施输出 Parquet/Flight SQL，不能在 PACK 中临时安装依赖。

每次写入后：

1. 重新执行 inspection，确认 Source Guide、Sources 与 Workflows 的生产 Interface。
2. 用普通单测验证 Decoder、Parser 与 Analysis；用 `kat_run(sources=...)` 验证真实 Source Input Compiler、Source Resolution、Analysis 和 Workflow 链路。
3. 运行适用的 `kat test`；失败时使用报告和日志诊断，但不把失败说成完成。
4. 交付变更摘要、受影响文件、实际验证证据和仍存限制。

“诊断失败”本身不授权修复。无法在现有授权和事实下继续时，按 [result-contract.md](result-contract.md) 交付最小下一步。
