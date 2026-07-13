# kat Python SDK/Runtime 与 DataFusion 54 对齐设计

## 1. 结论

本设计补充并修正 `2026-07-09-kat-rs-python-design.md` 中与 Python SDK、Runtime 和 DataFusion Python API 相关的实现合同。

最终依赖方向为：

```text
Python Pack
  -> kat-python-sdk

kat-python-runtime
  -> kat-python-sdk
  -> datafusion == 54.0.0

Rust CLI / Rust DataFusion
  -> 仅通过 catalog.json + Parquet 与 Python worker 交换数据
```

`kat-python-sdk` 不增加 DataFusion 运行时硬依赖；`kat-python-runtime` 持有 DataFusion 引擎依赖。`Kat.sql()` 直接使用 DataFusion 54 原生 `$name` 参数与 `param_values`，删除自研 `:name` 正则替换。两个 Python 包必须能构建 wheel，并在干净 venv 中安装后运行真实 worker。

本次还增加一条真实端到端验收：从 `kat-rs` CLI 出发，在本地 `test/test.db` 上运行 `packs/openharmony-critical-path` 的 `wechat_first_frame_critical_path`，验证最终 Parquet artifacts。

## 2. 问题与现状

当前实现已经存在 DataFusion 主链路，并非完全缺少对应代码：

- `kat-python-runtime` 已声明 `datafusion` 并在 Run Worker 中创建 `SessionContext`。
- Runtime 已按 `catalog.json` 注册 Parquet，并调用 `DataFrame.write_parquet()` 物化 workflow 返回值。
- 当前 Python 合同测试能通过源码 `PYTHONPATH` 跑通 toy worker。

真正缺口是：

1. Runtime 实际 import `kat`，但 wheel 元数据没有声明 `kat-python-sdk`，源码 `PYTHONPATH` 掩盖了依赖缺口。
2. `datafusion` 没有版本约束，无法保证使用已验证的 54 API。
3. SDK 用 `Any` 隐藏 `SessionContext` / `DataFrame` 合同，并自行用正则把 `:name` 渲染成 SQL 字面量。
4. Runtime 只通过 `hasattr(value, "write_parquet")` 判断 DataFrame，不能落实 `dict[str, DataFrame]` 合同。
5. Dataset registration 只做了部分路径检查，没有完整拒绝重复表名、缺失文件和规范化后越界。
6. Python 测试直接挂源码，CI 不构建或安装 wheel，也不运行 Python worker。
7. CLI 自动把仓库 SDK/Runtime 目录写入 `PYTHONPATH`，使 clean-venv 验收仍可能误用源码。
8. 旧设计中的 `DataFrame.register_view()` 与 DataFusion 54 API 不符；正确入口是 `SessionContext.register_view(name, df)`，或 `DataFrame.into_view()` 后注册。

## 3. 目标

1. 固定并验证 DataFusion Python 54.0.0 的 API 合同。
2. 保持 SDK authoring surface 轻量，同时提供可发布的 DataFusion 类型信息。
3. 让 Runtime wheel 声明完整依赖闭包。
4. 删除自研 SQL 参数序列化，统一使用 DataFusion 原生标量绑定。
5. 精确校验 workflow 返回的真实 DataFusion DataFrame。
6. 加强 Python 侧 catalog 注册边界，但不复制 Parquet 解析器。
7. 通过两个本地 wheel、干净 venv 和真实 worker 证明安装合同。
8. 通过 `kat-rs` CLI、真实 `test/test.db` 和示例 Pack 证明端到端主路径。
9. 同步迁移当前 Pack 以及外部 Pack 中不符合 DataFusion 54 的参数和 view API。

## 4. 非目标

本切片不做：

- 不实现 bundled CPython 或离线 wheelhouse。
- 不引入新的 Python workspace、依赖管理器或 Pack dependency resolver。
- 不让 `kat-python-sdk` 硬依赖 DataFusion。
- 不增加 `:name` 兼容翻译、弃用告警或双语法窗口。
- 不重包 DataFusion DataFrame API，也不新增 kat 自研 DataFrame。
- 不实现 SQL 审计日志、`logs.jsonl`、artifact preview 或 row-count metadata。
- 不实现 native operators 或填充当前空的 `kat.operators`。
- 不增加 artifact 事务、回滚、原子替换或失败现场清理。
- 不要求 Rust DataFusion 53.1.x 与 Python DataFusion 54.0.0 使用相同内部版本。
- 不把 `test/test.db`、venv、wheel、临时 dataset、run 目录或 artifacts 纳入交付。

## 5. 方案比较

### 5.1 采用：Runtime 持有引擎依赖，SDK 提供类型化薄门面

- Runtime 精确依赖 SDK 与 DataFusion 54。
- SDK 只在类型检查阶段引用 DataFusion 类型。
- Pack 日常只 import `kat`，实际数据对象仍是 DataFusion DataFrame。

这个方案符合既有依赖方向，改动集中，不新增中间抽象。

### 5.2 不采用：SDK 直接依赖 DataFusion

优点是类型引用直接，但会让只使用 decorators 或 discovery 的 SDK 安装也携带重型 native wheel，并把运行时引擎依赖上移到 authoring SDK。

### 5.3 不采用：新增 DataFusion adapter/contracts 包

独立包可以进一步隔离类型，但当前只有一个引擎实现，会额外增加第三个 wheel、版本联动和发布关系，超出 MVP 需要。

## 6. 包与依赖合同

### 6.1 `kat-python-sdk`

`pyproject.toml` 保持无运行时 dependencies，并显式声明 PEP 517 build backend：

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[tool.setuptools.package-data]
kat = ["py.typed"]
```

SDK 使用 `from __future__ import annotations` 和 `TYPE_CHECKING` 表达类型：

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from datafusion import DataFrame, SessionContext
```

公开类型合同至少包括：

```python
class Kat:
    ctx: SessionContext

    def __init__(self, *, ctx: SessionContext, ...) -> None: ...
    def sql(self, sql: str, **params: Any) -> DataFrame: ...
```

SDK wheel 包含 `kat/py.typed`，让 Pack authoring 环境可以读取这些标注。类型检查环境需要同时安装 Runtime 或 DataFusion，但 SDK import 本身不加载 DataFusion native module。

### 6.2 `kat-python-runtime`

Runtime wheel 显式依赖：

```toml
dependencies = [
  "kat-python-sdk==0.1.0",
  "datafusion==54.0.0",
]
```

SDK 与 Runtime 在当前未发布 MVP 中保持相同的 `0.1.0` 版本。后续任一包升级公共合同，两者同步升级，Runtime 始终精确依赖对应 SDK 版本。

PyArrow 继续由 DataFusion 54 的依赖闭包提供。本设计不因为某个 Pack 直接使用 PyArrow 就把 PyArrow 错误地加入 SDK；Pack 依赖声明机制另行设计。

### 6.3 安装合同

验收先分别构建 SDK 和 Runtime wheel，再在干净 venv 中从本地 wheel 目录安装 Runtime wheel。Runtime 对 SDK 的依赖必须由同一目录中的 SDK wheel满足，随后运行 `pip check`。

源码 `PYTHONPATH` 模式可以由开发者显式配置，但不能作为 wheel 依赖正确性的验收证据。

## 7. SDK SQL 与 DataFusion API 合同

`Kat.sql()` 直接委托 DataFusion：

```python
def sql(self, sql: str, **params: Any) -> DataFrame:
    return self.ctx.sql(sql, param_values=params or None)
```

规则：

- 标量占位符统一使用 `$name`。
- `kat.sql()` 只做标量参数绑定，不提供表名或列名模板替换。
- 参数转换、类型保持与错误由 DataFusion/PyArrow 原生实现负责。
- 删除 `_bind_sql_params()`、`_sql_literal()` 和所有自研 SQL 字面量代码。
- 不保留 `:name` 兼容路径；遗留旧语法是 Pack 迁移错误。
- 高级代码确实需要 DataFrame 或标识符替换时，可以显式调用 `kat.ctx.sql()` 的上游 API，不扩张 `kat.sql()` 日常合同。
- 临时 view 使用 `kat.ctx.register_view(name, df)`；不使用不存在的 `df.register_view()`。
- `SessionContext.from_arrow()`、`from_pydict()`、`DataFrame.write_parquet()` 等 DataFusion 54 原生能力保持可用。

## 8. Runtime 数据流

Run Worker 保持单进程、单 dataset Session：

```text
read request
  -> create SessionContext
  -> validate catalog
  -> register Parquet tables
  -> load Pack and select workflow
  -> build Kat(ctx)
  -> bind inputs
  -> execute workflow
  -> validate dict[str, DataFrame]
  -> materialize artifacts
  -> write manifest
```

`worker/run.py` 只负责编排；catalog 细节留在 `dataset.py`，返回合同与写出留在 `artifacts.py`。不新增 QueryResult、Arrow IPC、Flight 或跨进程 DataFusion 对象。

## 9. Dataset registration

`register_dataset(ctx: SessionContext, dataset_path: Path)` 校验：

1. `catalog.json` 顶层必须是对象，`tables` 必须是数组。
2. 每张表必须提供非空字符串 `name` 与 `path`。
3. 表名不得重复；不增加比 Rust dataset reader 更严格的字符白名单。
4. 路径必须相对、不得包含父目录分量。
5. 文件必须存在；规范化后的路径必须仍位于规范化后的 dataset 根目录内。
6. 最后调用 `ctx.register_parquet(name, path)`，由 DataFusion 负责 Parquet 读取与格式错误。

错误必须附带表名和路径上下文。Python 侧只重复跨进程注册所需的薄校验，不自研 catalog 框架或 Parquet metadata parser。

## 10. Workflow 返回与 artifact 合同

Artifact 分为两个阶段：

### 10.1 完整校验

在写任何文件前验证：

- 返回值是 `dict`。
- 每个 key 是符合现有安全规则的 artifact name。
- 每个 value 都是 `isinstance(value, datafusion.DataFrame)`。
- 所有目标路径均不存在；MVP 采用拒绝覆盖。

只拥有同名 `write_parquet` 方法的伪对象必须被拒绝。如果任一元素非法，不能写出部分 artifacts。

### 10.2 物化

完整校验后逐个调用 `DataFrame.write_parquet()`。DataFusion 惰性计划在此阶段真正执行。

如果后续写出失败，run 标记失败并保留已经产生的现场；本切片不增加清理、事务或回滚。Manifest 中继续只记录 artifact `name` 与相对 `path`。

## 11. 错误分类

不增加复杂异常继承树。Worker 按执行阶段生成稳定 `error.kind`，同时保留原始异常类型、消息和 traceback：

```json
{
  "status": "failed",
  "error": {
    "kind": "dataset_registration",
    "type": "Exception",
    "message": "...",
    "traceback": "..."
  }
}
```

稳定分类：

| `error.kind` | 含义 |
| --- | --- |
| `request_contract` | request JSON 损坏、字段缺失或类型错误 |
| `session_creation` | DataFusion SessionContext 初始化失败 |
| `dataset_registration` | catalog 校验或 Parquet 注册失败 |
| `pack_load` | Pack import 失败 |
| `workflow_selection` | workflow 不存在或重名 |
| `input_contract` | inputs 无法绑定到 workflow 签名 |
| `workflow_execution` | workflow 函数执行期间抛错 |
| `return_contract` | 返回值不满足 `dict[str, DataFrame]` |
| `materialization` | DataFrame 惰性执行或 Parquet 写出失败 |

调用 workflow 前使用 `inspect.signature(workflow).bind(kat, **inputs)`，把签名错误与 workflow 内部 `TypeError` 分开。

Request 无法提供有效 `runDir` 时，以 request 文件所在目录作为失败 manifest 回退目录。成功退出码为 `0`，失败为 `1`。如果回退目录本身也不可写，worker 只向 stderr 报错并非零退出，不创建第二套持久化位置。

## 12. Pack 迁移

当前仓库 `packs/openharmony-critical-path` 与外部 Pack 使用同一迁移合同：

- `:name` 全部改为 `$name`。
- 标量查询通过 `kat.sql(..., name=value)`，最终进入 `param_values`。
- `DataFrame.register_view()` 改为 `SessionContext.register_view(name, df)`。
- 继续使用真实 DataFusion DataFrame，不增加 kat wrapper。
- 在 DataFusion 54 环境运行各自测试，并扫描遗留旧参数和错误 view API。

本仓库 PR 只提交本仓库 Pack 的修改。用户在另一任务或仓库维护的外部 Pack 同步应用相同合同；其测试结果作为整体迁移证据，但外部仓库文件不混入本仓库提交。

## 13. CLI Python 环境合同

当前 CLI 无条件注入仓库 SDK/Runtime 源码路径，会让 clean-venv E2E 无法证明 wheel 被使用。因此本切片包含一项必要的 CLI 调整：

- `KAT_RS_PYTHON` 只负责选择 Python 解释器。
- CLI 不再合成或注入仓库源码 `PYTHONPATH`。
- 进程可以正常继承调用方显式设置的环境，但 clean-venv 验收必须清空 `PYTHONPATH`。
- 开发者需要源码模式时自行显式设置 `PYTHONPATH`；正式验收使用已安装 wheel。

本切片不顺带实现 bundled CPython，也不改造 CLI summary 形式。

## 14. 测试策略

### 14.1 SDK/DataFusion 合同

使用真实 DataFusion 54 `SessionContext` 验证：

- `$id` / `$id2` 等重叠参数。
- 整数、浮点、布尔、`None` 和含引号字符串。
- `Kat.sql()` 返回真实 `datafusion.DataFrame`。
- 删除所有自研 SQL 字面量和正则替换测试。

### 14.2 Runtime 单元与集成

Dataset registration 覆盖合法 catalog、重复表名、绝对路径、父目录、规范化后越界、缺失文件和 Parquet 注册失败。

Artifact 覆盖真实 DataFrame、伪 `write_parquet` 对象、全量预校验、拒绝覆盖和物化失败。Worker 覆盖主要 `error.kind`，特别是 input、return、dataset 和 materialization。

Toy worker 集成测试必须重新查询输出 Parquet 内容，不能只断言路径存在。

### 14.3 Wheel / clean-venv

```text
build SDK wheel
build Runtime wheel
create clean venv
install Runtime wheel from local dist directory
pip check
assert datafusion.__version__ == "54.0.0"
run discovery worker
run real worker on synthetic Parquet dataset
query generated artifact
```

CI 在 Windows 与 Linux 各运行一次 clean-venv 验收。验证时检查 `kat` 和 `kat_runtime` 的 module path 位于 venv `site-packages`，而不是仓库源码目录。

### 14.4 真实 `test/test.db` CLI E2E

端到端路径必须从 `kat-rs` 二进制开始：

```text
test/test.db
  -> kat-rs dataset materialize sqlite
  -> local catalog + Parquet dataset
  -> kat-rs pack inspect packs/openharmony-critical-path --json
  -> kat-rs pack run ... wechat_first_frame_critical_path
  -> clean-venv SDK/Runtime/DataFusion 54
  -> path_nodes.parquet + path_edges.parquet + success manifest
```

测试放在 `kat-rs-cli` 集成测试中，调用真实 `kat-rs` 二进制。`KAT_RS_PYTHON` 指向已安装两个 wheel 的 clean venv；`KAT_RS_E2E_DB` 可以覆盖输入路径，默认验收输入为仓库根目录下未跟踪的 `test/test.db`。

测试断言：

1. `dataset materialize sqlite`、`pack inspect`、`pack run` 都以 `0` 退出。
2. Discovery 能看到 `wechat_first_frame_critical_path`。
3. Catalog 包含 `thread_state`、`instant`、`thread`、`process`、`callstack`、`frame_slice` 等事实表。
4. Run manifest 状态为 `success`。
5. `path_nodes.parquet`、`path_edges.parquet` 存在并可由 DataFusion 54 查询。
6. `path_nodes` 非空，schema 符合关键路径合同，并命中 `.tencent.wechat` 首帧目标事实。
7. 对所有非空 wakeup edges，回查 `instant` 后不存在无法对应的边；缺少证据时保持 uncertainty。
8. `kat` 与 `kat_runtime` 实际来自 clean venv 的 `site-packages`。

真实数据库不进入 CI 或版本库。CI 使用合成 fixture；真实 E2E 是显式的本地或发布前测试，并在 PR 中记录命令、版本和结果。

## 15. 最小实现切片

1. 先写失败测试，覆盖 wheel 依赖闭包、DataFusion 原生参数和真实 DataFrame 判定。
2. 补齐两个 `pyproject.toml` 的 build/依赖配置和 SDK 类型标注。
3. 删除 SDK 自研 SQL 绑定，迁移当前 Pack 到 `$name`。
4. 加强 dataset registration 和 artifact 两阶段合同。
5. 实现稳定错误分类与 input signature 预绑定。
6. 移除 CLI 自动源码 `PYTHONPATH` 注入，并调整相关 CLI 测试。
7. 增加 wheel / clean-venv worker 验收。
8. 增加并实际执行 `test/test.db` + `openharmony-critical-path` CLI E2E。
9. 同步核对外部 Pack 的 DataFusion 54 迁移证据。

每一步只修改本目标所需文件，不顺手实现日志、operators、bundled Python 或其他设计欠账。

## 16. 验收标准

- SDK 与 Runtime wheel 均能构建。
- 干净 venv 仅从 wheel 安装后 `pip check` 通过。
- Runtime 安装得到 `kat-python-sdk==0.1.0` 和 `datafusion==54.0.0`。
- SDK import 不在运行时加载 DataFusion，但公开类型明确表达 `SessionContext` 和 `DataFrame`。
- `Kat.sql()` 使用 `$name + param_values`，仓库 Pack 不再含 `:name` 参数。
- Runtime 只接受真实 DataFusion DataFrame，且非法返回不会产生部分 artifacts。
- Dataset registration 拒绝重复表、路径越界和缺失文件。
- Worker manifest 使用稳定的阶段错误分类。
- Windows 与 Linux clean-venv worker 测试通过。
- 真实 `test/test.db` 通过 `kat-rs` CLI 完成 materialize、inspect 和 Pack Run。
- `path_nodes` / `path_edges` 可查询，节点和 wakeup edge 满足上述事实约束。
- 外部 Pack 已按同一 DataFusion 54 合同完成同步检查。
- PR 只包含本次最小切片的源码、测试和设计文档，不包含 wheel、venv、数据库副本或运行产物。

## 17. 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| `$name` 是一次破坏性 Pack 语法切换 | 当前仓库和外部 Pack 同一交付窗口迁移，不维护双语法 |
| CLI 仍可能通过外部 `PYTHONPATH` 误用源码 | clean-venv 验收显式清空 `PYTHONPATH` 并断言 module path |
| DataFusion 惰性错误直到写 artifact 才出现 | 明确归类为 `materialization` 并保留 traceback |
| Python/Rust DataFusion 版本不同 | 进程边界只交换 catalog 与 Parquet，不交换内部对象 |
| 真实数据库 E2E 不适合普通 CI | 提交可重复执行的集成测试，本地/发布前运行并记录证据 |
| 外部 Pack 不属于本仓库提交范围 | 使用相同迁移合同和独立测试证据，不把跨仓库文件混入当前 PR |

## 18. 相关设计

- [短命 CLI 与 Python Pack Runtime 重写设计](2026-07-09-kat-rs-python-design.md)
- [Pack Run MVP 实现设计](2026-07-09-kat-rs-pack-run-mvp-implementation-design.md)
- [通用关键路径示例 Pack 重构设计](2026-07-10-kat-rs-critical-path-pack-redesign.md)
