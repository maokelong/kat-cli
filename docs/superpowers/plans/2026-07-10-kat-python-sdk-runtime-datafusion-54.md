# Kat Python SDK/Runtime DataFusion 54 Native Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `kat-python-sdk`、`kat-python-runtime`、仓库内 OpenHarmony 示例 Pack 与 `kat-rs` CLI 形成可由本地 wheel 安装、原生使用 DataFusion 54、并经过合成数据和真实 `test.db` 验证的闭环。

**Architecture:** SDK 只暴露 `Kat` authoring API，并以延迟类型标注表达 `SessionContext`/`DataFrame`，不在运行时依赖 DataFusion；Runtime 精确依赖 SDK 0.1.0 和 DataFusion 54.0.0，负责 catalog 注册、workflow 阶段编排、真实 DataFrame 校验和 Parquet 物化。Rust CLI 只选择 `KAT_RS_PYTHON` 并启动已安装的 worker，不再把仓库源码写入 `PYTHONPATH`；进程之间仍只交换 JSON、catalog 和 Parquet。

**Tech Stack:** Python >=3.10、setuptools PEP 517、datafusion==54.0.0、PyArrow（由 DataFusion 依赖闭包提供）、pytest、Rust 2024、clap、serde_json、tempfile、GitHub Actions Windows/Linux matrix。

## Global Constraints

- `kat-python-runtime==0.1.0` 必须精确依赖 `kat-python-sdk==0.1.0` 和 `datafusion==54.0.0`。
- `kat-python-sdk` 不得声明或运行时导入 DataFusion；仅在 `TYPE_CHECKING` 下导入 `SessionContext` 和 `DataFrame`，并分发 `py.typed`。
- `Kat.sql()` 只支持 DataFusion 54 原生 `$name` + `param_values`，不保留 `:name` 兼容层或 SQL 字面量拼接。
- 临时 view 的合同是 `SessionContext.register_view(name, df)` 或 `DataFrame.into_view()`；本切片不新增 view 抽象。
- Runtime 只接受真实 `datafusion.DataFrame` artifact；artifact name 完整匹配 `[A-Za-z0-9_][A-Za-z0-9_-]*` 并映射到 `artifacts/{name}.parquet`；所有 artifact 在任何写入前完成整体验证，已存在目标必须拒绝覆盖。
- 非 dict、非法 artifact name 和非 DataFrame 归入 `return_contract`；目标已存在、目录创建、惰性执行和 Parquet 写失败归入 `materialization`。
- Worker 的稳定 `error.kind` 恰为 `request_contract`、`session_creation`、`dataset_registration`、`pack_load`、`workflow_selection`、`input_contract`、`workflow_execution`、`return_contract`、`materialization`。
- Rust DataFusion 保持 53.1.0；Python/Rust DataFusion 不共享内部对象，只通过 catalog 和 Parquet 交互。
- `KAT_RS_PYTHON` 是 CLI 的 Python 解释器选择入口；CLI 不修改 `PYTHONPATH`，clean-venv 验收显式清除 `PYTHONPATH`/`PYTHONHOME`。
- 不实现 bundled CPython、日志/`logs.jsonl`、artifact preview/row count、native operators、事务回滚、原子替换、旧 SQL 兼容层或新的依赖管理器。
- 仓库外 Pack 由另一任务维护，本仓库交付不修改也不等待其文件；不把外部仓库证据作为本 PR 门禁，但在外部仓库身份、提交和测试证据齐备前，整体 DataFusion 54 迁移状态必须保持 pending。
- 真实数据库门禁只接受 SHA-256 `5F742A759C57BB05FE010E44A1F03AA042E4B7CF6EE53769FA55F7CFD6FE8829` 的 `test.db`；`KAT_RS_E2E_DB` 只允许改变该文件的位置，不允许替换输入内容。
- 真实数据库测试是显式 `#[ignore]` 的本地/发布前门禁：主动运行时缺文件、哈希不匹配或断言失败都必须失败；普通 CI 不因缺少真实数据库而跳过已请求的测试，也不自动运行它。
- CI 的独立门禁是合成 fixture Python 测试与 Windows/Linux clean-wheel 测试；真实数据库结果在 PR/发布记录中单独附命令、Python/DataFusion 版本和结果。
- 工作树可能包含用户的无关修改；每个提交只暂存本任务列出的文件，不删除、还原或格式化无关文件。

---

## File Structure

- `python/kat-python-sdk/kat/context.py`：SDK 的 `Kat` 上下文、DataFusion 延迟类型和原生 SQL 参数委托。
- `python/kat-python-sdk/kat/py.typed`：PEP 561 类型标记。
- `python/kat-python-sdk/pyproject.toml`：SDK PEP 517 构建和 package-data 配置。
- `python/kat-python-runtime/pyproject.toml`：Runtime PEP 517 构建及精确运行时依赖。
- `python/kat-python-runtime/kat_runtime/dataset.py`：catalog 与 dataset 内 Parquet 路径校验、表注册。
- `python/kat-python-runtime/kat_runtime/artifacts.py`：artifact 整体验证计划与物化。
- `python/kat-python-runtime/kat_runtime/pack_loader.py`：Pack import/discovery 与 workflow 唯一选择。
- `python/kat-python-runtime/kat_runtime/worker/run.py`：请求读取、九阶段编排、manifest 和退出码。
- `packs/openharmony-critical-path/facts/{callstacks,frames,scheduling,threads}.py`：迁移到 `$name` 参数语法。
- `python/tests/test_sdk_runtime_contract.py`：SDK 原生 SQL、discovery 与成功 worker 合同。
- `python/tests/test_runtime_validation.py`：dataset 与 artifact 的纯合同测试。
- `python/tests/test_worker_error_contract.py`：九种稳定错误阶段和 manifest 测试。
- `python/tests/wheel_smoke.py`：在已安装 Runtime 的 venv 内运行 discovery/worker/query smoke。
- `python/tests/verify_wheels.py`：跨平台构建 wheel、创建 SDK-only/Runtime venv、安装并调用 smoke。
- `python/tests/verify_cli_e2e.py`：由 Rust 集成测试调用，验证真实 DB 指纹、module provenance、artifact schema/事实。
- `crates/kat-rs-cli/src/python_worker.rs`：解释器选择和 Python 子进程启动，不再注入源码路径。
- `crates/kat-rs-cli/tests/pack_run_contract.rs`：请求序列化与参数解析合同；移除无行为的 E2E 占位。
- `crates/kat-rs-cli/tests/pack_run_e2e.rs`：合成 SQLite 与真实 `test.db` 的 `kat-rs` 二进制 E2E 编排。
- `.github/workflows/ci.yml`：Ubuntu Python 合同回归。
- `.github/workflows/full-ci.yml`：Windows/Linux clean-wheel 验收。

## Preflight

- [ ] **Step 1: Confirm the execution baseline**

Run from repository root:

```powershell
python --version
python -c "import datafusion; assert datafusion.__version__ == '54.0.0'; print(datafusion.__version__)"
cargo --version
```

Expected: Python is at least 3.10, the second command prints `54.0.0`, and Cargo is available.

- [ ] **Step 2: Install plan-only test/build tooling when absent**

```powershell
python -m pip install --upgrade build pytest
```

Expected: `python -m build --version` and `python -m pytest --version` both exit 0. This does not install either kat package from source.

### Task 1: Package Metadata and Native SDK SQL Contract

**Files:**
- Create: `python/kat-python-sdk/kat/py.typed`
- Modify: `python/kat-python-sdk/kat/context.py`
- Modify: `python/kat-python-sdk/pyproject.toml`
- Modify: `python/kat-python-runtime/pyproject.toml`
- Test: `python/tests/test_sdk_runtime_contract.py`

**Interfaces:**
- Consumes: DataFusion 54 `SessionContext.sql(query, param_values=...)`.
- Produces: `Kat.__init__(ctx: SessionContext, ...)`, `Kat.sql(sql: str, **params: Any) -> DataFrame`, SDK wheel with `py.typed`, Runtime metadata with exact dependencies.

- [ ] **Step 1: Replace the old binder test with failing native-delegation tests**

In `python/tests/test_sdk_runtime_contract.py`, replace `test_sql_binding_replaces_overlapping_parameters_atomically` with:

```python
def test_kat_sql_delegates_query_and_param_values_without_rewriting():
    sys.path.insert(0, str(SDK_ROOT))
    from kat import Kat

    calls = []

    class CapturingContext:
        def sql(self, query, *, param_values):
            calls.append((query, param_values))
            return "dataframe"

    kat = Kat(ctx=CapturingContext())
    result = kat.sql("select $id as id, $id2 as id2", id=1, id2=2)

    assert result == "dataframe"
    assert calls == [
        ("select $id as id, $id2 as id2", {"id": 1, "id2": 2})
    ]


def test_kat_sql_uses_real_datafusion_54_parameters():
    sys.path.insert(0, str(SDK_ROOT))
    from datafusion import DataFrame, SessionContext
    from kat import Kat

    dataframe = Kat(ctx=SessionContext()).sql(
        """
        select $id as id,
               $id2 as id2,
               $quoted as quoted,
               $missing is null as missing,
               $flag as flag,
               $ratio as ratio
        """,
        id=1,
        id2=2,
        quoted="O'Reilly",
        missing=None,
        flag=True,
        ratio=1.5,
    )

    assert isinstance(dataframe, DataFrame)
    assert dataframe.to_pydict() == {
        "id": [1],
        "id2": [2],
        "quoted": ["O'Reilly"],
        "missing": [True],
        "flag": [True],
        "ratio": [1.5],
    }


def test_sdk_exposes_deferred_datafusion_types_without_runtime_import():
    sys.path.insert(0, str(SDK_ROOT))
    from kat import Kat
    import kat.context as context

    assert Kat.__init__.__annotations__["ctx"] == "SessionContext"
    assert Kat.sql.__annotations__["return"] == "DataFrame"
    assert not hasattr(context, "_bind_sql_params")
    assert not hasattr(context, "_sql_literal")
```

- [ ] **Step 2: Run the native SQL tests and observe the old implementation fail**

```powershell
python -m pytest python/tests/test_sdk_runtime_contract.py -k "kat_sql or deferred_datafusion" -q -p no:cacheprovider --basetemp .pytest-tmp/sdk-red
```

Expected: FAIL because the current context rewrites `:name`, calls `ctx.sql()` without `param_values`, returns `Any`, and still exports the private binder helpers.

- [ ] **Step 3: Replace `kat/context.py` with the minimal native contract**

```python
from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from datafusion import DataFrame, SessionContext


class Kat:
    def __init__(
        self,
        *,
        ctx: SessionContext,
        run_dir: str | None = None,
        logger: Any = None,
    ) -> None:
        self.ctx = ctx
        self.run_dir = run_dir
        self._logger = logger

    def sql(self, sql: str, **params: Any) -> DataFrame:
        return self.ctx.sql(sql, param_values=params or None)

    def log(self, level: str, message: str, **fields: Any) -> None:
        if self._logger is not None:
            self._logger(level, message, fields)
```

Create an empty `python/kat-python-sdk/kat/py.typed` file.

- [ ] **Step 4: Make both projects explicit PEP 517 packages with exact dependencies**

Replace `python/kat-python-sdk/pyproject.toml` with:

```toml
[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"

[project]
name = "kat-python-sdk"
version = "0.1.0"
requires-python = ">=3.10"

[tool.setuptools.packages.find]
where = ["."]
include = ["kat*"]

[tool.setuptools.package-data]
kat = ["py.typed"]
```

Replace `python/kat-python-runtime/pyproject.toml` with:

```toml
[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"

[project]
name = "kat-python-runtime"
version = "0.1.0"
requires-python = ">=3.10"
dependencies = [
  "kat-python-sdk==0.1.0",
  "datafusion==54.0.0",
]

[tool.setuptools.packages.find]
where = ["."]
include = ["kat_runtime*"]
```

- [ ] **Step 5: Run focused tests and build both wheels**

```powershell
python -m pytest python/tests/test_sdk_runtime_contract.py -k "kat_sql or deferred_datafusion" -q -p no:cacheprovider --basetemp .pytest-tmp/sdk-green
$dist = Join-Path $env:TEMP 'kat-rs-plan-task1-dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
python -m build --wheel --outdir $dist python/kat-python-sdk
python -m build --wheel --outdir $dist python/kat-python-runtime
```

Expected: focused tests pass and `$dist` contains one SDK wheel and one Runtime wheel. Do not add those wheels to Git.

- [ ] **Step 6: Commit the package and SDK slice**

```powershell
git add python/kat-python-sdk/kat/context.py python/kat-python-sdk/kat/py.typed python/kat-python-sdk/pyproject.toml python/kat-python-runtime/pyproject.toml python/tests/test_sdk_runtime_contract.py
git commit -m "feat(python): 使用 DataFusion 54 原生 SQL 合同"
```

### Task 2: Migrate the Repository Pack to `$name`

**Files:**
- Modify: `packs/openharmony-critical-path/facts/callstacks.py`
- Modify: `packs/openharmony-critical-path/facts/frames.py`
- Modify: `packs/openharmony-critical-path/facts/scheduling.py`
- Modify: `packs/openharmony-critical-path/facts/threads.py`
- Test: `python/tests/test_openharmony_critical_path_pack.py`

**Interfaces:**
- Consumes: Task 1 `Kat.sql(sql, **params)` native binding.
- Produces: repository Pack SQL containing only DataFusion 54 `$name` placeholders.

- [ ] **Step 1: Add a source-level regression test for the known parameter names**

Add to `python/tests/test_openharmony_critical_path_pack.py`:

```python
def test_pack_sources_do_not_use_legacy_colon_parameters():
    fact_sources = [
        (PACK_ROOT / "facts" / name).read_text(encoding="utf-8")
        for name in ["callstacks.py", "frames.py", "scheduling.py", "threads.py"]
    ]
    legacy_parameters = [
        "app_name",
        "end_ts",
        "itid",
        "start_ts",
        "target_itid",
    ]

    for source in fact_sources:
        for parameter in legacy_parameters:
            assert f":{parameter}" not in source
```

- [ ] **Step 2: Run the regression test and observe every legacy query fail the contract**

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py::test_pack_sources_do_not_use_legacy_colon_parameters -q -p no:cacheprovider --basetemp .pytest-tmp/pack-red
```

Expected: FAIL and identify at least one `:name` occurrence.

- [ ] **Step 3: Mechanically replace only SQL placeholders**

Apply these exact replacements inside the four SQL strings, leaving Python annotations and keyword arguments unchanged:

```text
:app_name     -> $app_name
:end_ts       -> $end_ts
:itid         -> $itid
:start_ts     -> $start_ts
:target_itid  -> $target_itid
```

The resulting `first_frame_window` predicate, for example, must read:

```python
        where p.name = $app_name
          and f.type = 0
          and f.dur > 0
```

The resulting wakeup bounds must read:

```python
          and ref = $target_itid
          and ts between $start_ts and $end_ts
```

- [ ] **Step 4: Run Pack fact/workflow regression and scan the repository Pack**

```powershell
python -m pytest python/tests/test_openharmony_critical_path_pack.py -k "pack_sources or facts or workflow" -q -p no:cacheprovider --basetemp .pytest-tmp/pack-green
rg -n ":[A-Za-z_][A-Za-z0-9_]*" packs/openharmony-critical-path -g "*.py"
```

Expected: pytest passes. `rg` exits 1 with no matches; this exit code means the legacy syntax is absent.

- [ ] **Step 5: Commit the Pack migration**

```powershell
git add packs/openharmony-critical-path/facts/callstacks.py packs/openharmony-critical-path/facts/frames.py packs/openharmony-critical-path/facts/scheduling.py packs/openharmony-critical-path/facts/threads.py python/tests/test_openharmony_critical_path_pack.py
git commit -m "feat(pack): 迁移到 DataFusion 54 参数语法"
```

### Task 3: Dataset Registration and Artifact Preflight

**Files:**
- Modify: `python/kat-python-runtime/kat_runtime/dataset.py`
- Modify: `python/kat-python-runtime/kat_runtime/artifacts.py`
- Modify: `python/kat-python-runtime/kat_runtime/worker/run.py`
- Create: `python/tests/test_runtime_validation.py`

**Interfaces:**
- Consumes: DataFusion 54 `SessionContext`, `DataFrame`, `DataFrameWriteOptions`.
- Produces: `register_dataset(ctx: SessionContext, dataset_path: Path) -> None`, `validate_artifacts(result: object, run_dir: Path) -> list[ArtifactPlan]`, `materialize_artifacts(plans: list[ArtifactPlan]) -> list[dict[str, str]]`.

- [ ] **Step 1: Add failing dataset and artifact contract tests**

Create `python/tests/test_runtime_validation.py` with:

```python
import json
import sys
from pathlib import Path

import pytest
from datafusion import SessionContext


REPO_ROOT = Path(__file__).resolve().parents[2]
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
sys.path.insert(0, str(RUNTIME_ROOT))

from kat_runtime.artifacts import materialize_artifacts, validate_artifacts
from kat_runtime.dataset import register_dataset


def write_catalog(dataset: Path, tables: object) -> None:
    (dataset / "catalog.json").write_text(
        json.dumps({"tables": tables}),
        encoding="utf-8",
    )


def write_parquet(path: Path, value: int = 405) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    SessionContext().sql(f"select {value} as itid").write_parquet(str(path))


def test_register_dataset_registers_valid_parquet(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_parquet(dataset / "tables" / "thread.parquet")
    write_catalog(
        dataset,
        [{"name": "thread", "path": "tables/thread.parquet", "kind": "source"}],
    )
    ctx = SessionContext()

    register_dataset(ctx, dataset)

    assert ctx.table("thread").to_pydict() == {"itid": [405]}


@pytest.mark.parametrize(
    ("catalog", "message"),
    [
        ([], "catalog must be a JSON object"),
        ({"tables": {}}, "catalog tables must be a list"),
    ],
)
def test_register_dataset_rejects_invalid_catalog_shape(tmp_path, catalog, message):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    (dataset / "catalog.json").write_text(json.dumps(catalog), encoding="utf-8")

    with pytest.raises(TypeError, match=message):
        register_dataset(SessionContext(), dataset)


@pytest.mark.parametrize(
    ("tables", "message"),
    [
        (["not-an-object"], "must be an object"),
        ([{"name": "", "path": "tables/a.parquet"}], "non-empty name"),
        ([{"name": "a", "path": ""}], "non-empty path"),
        ([{"name": "a", "path": "../outside.parquet"}], "invalid relative path"),
        ([{"name": "a", "path": "tables/missing.parquet"}], "does not exist"),
    ],
)
def test_register_dataset_rejects_invalid_catalog_entries(tmp_path, tables, message):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_catalog(dataset, tables)

    with pytest.raises((TypeError, ValueError, FileNotFoundError), match=message):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_rejects_duplicate_table_names(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_parquet(dataset / "tables" / "a.parquet", 1)
    write_parquet(dataset / "tables" / "b.parquet", 2)
    write_catalog(
        dataset,
        [
            {"name": "duplicate", "path": "tables/a.parquet"},
            {"name": "duplicate", "path": "tables/b.parquet"},
        ],
    )

    with pytest.raises(ValueError, match="duplicate dataset table name"):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_rejects_absolute_and_symlink_escape(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    outside = tmp_path / "outside.parquet"
    write_parquet(outside)

    write_catalog(dataset, [{"name": "outside", "path": str(outside)}])
    with pytest.raises(ValueError, match="invalid relative path"):
        register_dataset(SessionContext(), dataset)

    link = dataset / "linked.parquet"
    try:
        link.symlink_to(outside)
    except OSError as error:
        pytest.skip(f"symlink creation is unavailable: {error}")
    write_catalog(dataset, [{"name": "outside", "path": "linked.parquet"}])
    with pytest.raises(ValueError, match="escapes dataset root"):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_rejects_non_parquet_file(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    bad = dataset / "tables" / "bad.parquet"
    bad.parent.mkdir()
    bad.write_text("not parquet", encoding="utf-8")
    write_catalog(dataset, [{"name": "bad", "path": "tables/bad.parquet"}])

    with pytest.raises(Exception, match="failed to register dataset table"):
        register_dataset(SessionContext(), dataset)


class FakeDataFrame:
    def write_parquet(self, path):
        Path(path).write_text("fake", encoding="utf-8")


@pytest.mark.parametrize(
    "name",
    ["", "-nodes", "path.nodes", "path/nodes", "path\\nodes", "路径"],
)
def test_validate_artifacts_rejects_unsafe_names_before_writing(tmp_path, name):
    dataframe = SessionContext().sql("select 1 as node_id")

    with pytest.raises(ValueError, match="invalid artifact name"):
        validate_artifacts({name: dataframe}, tmp_path / "run")

    assert not (tmp_path / "run" / "artifacts").exists()


def test_validate_artifacts_rejects_duck_typed_value_before_writing(tmp_path):
    result = {
        "valid": SessionContext().sql("select 1 as node_id"),
        "fake": FakeDataFrame(),
    }

    with pytest.raises(TypeError, match="not a DataFusion DataFrame"):
        validate_artifacts(result, tmp_path / "run")

    assert not (tmp_path / "run" / "artifacts").exists()


def test_materialize_artifacts_preflights_all_targets_before_writing(tmp_path):
    run_dir = tmp_path / "run"
    artifacts_dir = run_dir / "artifacts"
    artifacts_dir.mkdir(parents=True)
    (artifacts_dir / "second.parquet").write_text("keep", encoding="utf-8")
    ctx = SessionContext()
    plans = validate_artifacts(
        {
            "first": ctx.sql("select 1 as value"),
            "second": ctx.sql("select 2 as value"),
        },
        run_dir,
    )

    with pytest.raises(FileExistsError, match="artifact target already exists"):
        materialize_artifacts(plans)

    assert not (artifacts_dir / "first.parquet").exists()
    assert (artifacts_dir / "second.parquet").read_text(encoding="utf-8") == "keep"


def test_materialize_artifacts_writes_queryable_single_files(tmp_path):
    run_dir = tmp_path / "run"
    plans = validate_artifacts(
        {"path_nodes": SessionContext().sql("select 405 as itid, 'main' as name")},
        run_dir,
    )

    artifacts = materialize_artifacts(plans)
    artifact_path = run_dir / artifacts[0]["path"]
    reader = SessionContext()
    reader.register_parquet("path_nodes", str(artifact_path))

    assert artifacts == [
        {"name": "path_nodes", "path": "artifacts/path_nodes.parquet"}
    ]
    assert artifact_path.is_file()
    assert reader.table("path_nodes").to_pydict() == {
        "itid": [405],
        "name": ["main"],
    }
```

- [ ] **Step 2: Run the new tests and verify the current permissive implementation fails**

```powershell
python -m pytest python/tests/test_runtime_validation.py -q -p no:cacheprovider --basetemp .pytest-tmp/runtime-validation-red
```

Expected: FAIL because catalog shape/duplicate/canonical paths are unchecked, artifact validation is duck typed and interleaved with writes, and `validate_artifacts`/`ArtifactPlan` do not exist.

- [ ] **Step 3: Implement complete catalog validation before registration**

Replace `python/kat-python-runtime/kat_runtime/dataset.py` with:

```python
from __future__ import annotations

import json
from pathlib import Path

from datafusion import SessionContext


def register_dataset(ctx: SessionContext, dataset_path: Path) -> None:
    dataset_root = dataset_path.resolve(strict=True)
    catalog_path = dataset_root / "catalog.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    if not isinstance(catalog, dict):
        raise TypeError("dataset catalog must be a JSON object")
    tables = catalog.get("tables")
    if not isinstance(tables, list):
        raise TypeError("dataset catalog tables must be a list")

    seen_names: set[str] = set()
    validated: list[tuple[str, Path]] = []
    for index, table in enumerate(tables):
        if not isinstance(table, dict):
            raise TypeError(f"dataset table at index {index} must be an object")
        name = table.get("name")
        path_text = table.get("path")
        if not isinstance(name, str) or not name:
            raise ValueError(f"dataset table at index {index} requires a non-empty name")
        if not isinstance(path_text, str) or not path_text:
            raise ValueError(f"dataset table {name!r} requires a non-empty path")
        if name in seen_names:
            raise ValueError(f"duplicate dataset table name: {name}")
        seen_names.add(name)

        relative_path = Path(path_text)
        if relative_path.is_absolute() or relative_path.drive or ".." in relative_path.parts:
            raise ValueError(f"dataset table {name} has invalid relative path: {path_text}")
        candidate = dataset_root / relative_path
        if not candidate.exists():
            raise FileNotFoundError(f"dataset table {name} does not exist: {path_text}")
        parquet_path = candidate.resolve(strict=True)
        if not parquet_path.is_relative_to(dataset_root):
            raise ValueError(f"dataset table {name} escapes dataset root: {path_text}")
        if not parquet_path.is_file():
            raise ValueError(f"dataset table {name} is not a file: {path_text}")
        validated.append((name, parquet_path))

    for name, parquet_path in validated:
        try:
            ctx.register_parquet(name, str(parquet_path))
        except Exception as error:
            raise ValueError(f"failed to register dataset table {name}") from error
```

- [ ] **Step 4: Implement return validation and target/write preflight as separate phases**

Replace `python/kat-python-runtime/kat_runtime/artifacts.py` with:

```python
from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

from datafusion import DataFrame
from datafusion.dataframe import DataFrameWriteOptions


ARTIFACT_NAME = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_-]*")


@dataclass(frozen=True)
class ArtifactPlan:
    name: str
    dataframe: DataFrame
    path: Path
    relative_path: str


def validate_artifacts(result: object, run_dir: Path) -> list[ArtifactPlan]:
    if not isinstance(result, dict):
        raise TypeError("workflow must return dict[str, DataFrame]")

    plans: list[ArtifactPlan] = []
    for name, dataframe in result.items():
        if not isinstance(name, str) or ARTIFACT_NAME.fullmatch(name) is None:
            raise ValueError(f"invalid artifact name: {name!r}")
        if not isinstance(dataframe, DataFrame):
            raise TypeError(f"artifact {name} is not a DataFusion DataFrame")
        relative_path = f"artifacts/{name}.parquet"
        plans.append(
            ArtifactPlan(
                name=name,
                dataframe=dataframe,
                path=run_dir / relative_path,
                relative_path=relative_path,
            )
        )
    return plans


def materialize_artifacts(plans: list[ArtifactPlan]) -> list[dict[str, str]]:
    for plan in plans:
        if plan.path.exists():
            raise FileExistsError(f"artifact target already exists: {plan.path}")

    artifacts: list[dict[str, str]] = []
    write_options = DataFrameWriteOptions(single_file_output=True)
    for plan in plans:
        plan.path.parent.mkdir(parents=True, exist_ok=True)
        plan.dataframe.write_parquet(plan.path, write_options=write_options)
        artifacts.append({"name": plan.name, "path": plan.relative_path})
    return artifacts
```

This classifies invalid dict/name/value as `return_contract` in Task 4. Target existence, directory creation, lazy execution, and Parquet writes occur under `materialization`, while all target paths are still checked before the first write.

- [ ] **Step 5: Adapt the existing worker to the two-stage artifact interface**

In the current `run_request()` implementation in `python/kat-python-runtime/kat_runtime/worker/run.py`, replace the final line with:

```python
    plans = validate_artifacts(result, run_dir)
    return materialize_artifacts(plans)
```

Update its import to:

```python
from kat_runtime.artifacts import materialize_artifacts, validate_artifacts
```

This is the smallest bridge that keeps Task 3 independently green; Task 4 replaces the surrounding orchestration with explicit phases.

- [ ] **Step 6: Run focused validation and existing worker success tests**

```powershell
python -m pytest python/tests/test_runtime_validation.py python/tests/test_sdk_runtime_contract.py -k "register_dataset or artifact or run_worker" -q -p no:cacheprovider --basetemp .pytest-tmp/runtime-validation-green
```

Expected: all selected tests pass and the existing worker writes both artifacts through the new interface.

- [ ] **Step 7: Commit the validation slice**

```powershell
git add python/kat-python-runtime/kat_runtime/dataset.py python/kat-python-runtime/kat_runtime/artifacts.py python/kat-python-runtime/kat_runtime/worker/run.py python/tests/test_runtime_validation.py
git commit -m "feat(runtime): 校验 dataset 与 DataFrame artifacts"
```

### Task 4: Stable Worker Phases and Workflow Selection

**Files:**
- Modify: `python/kat-python-runtime/kat_runtime/pack_loader.py`
- Modify: `python/kat-python-runtime/kat_runtime/worker/run.py`
- Create: `python/tests/test_worker_error_contract.py`
- Modify: `python/tests/test_sdk_runtime_contract.py`

**Interfaces:**
- Consumes: Task 3 `validate_artifacts()` and `materialize_artifacts()`.
- Produces: `find_workflow(modules: list[ModuleType], workflow_name: str)`, `run_request_file(request_path: Path) -> int`, failure manifest with stable phase and original exception details.

- [ ] **Step 1: Add a nine-kind worker contract test harness**

Create `python/tests/test_worker_error_contract.py` with:

```python
import json
import sys
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
sys.path[:0] = [str(SDK_ROOT), str(RUNTIME_ROOT)]

import kat_runtime.worker.run as worker


def write_request(tmp_path: Path, *, inputs=None) -> tuple[Path, Path]:
    run_dir = tmp_path / "run"
    request_path = tmp_path / "request.json"
    request_path.write_text(
        json.dumps(
            {
                "packRoot": str(tmp_path / "pack"),
                "workflow": "sample",
                "datasetPath": str(tmp_path / "dataset"),
                "runDir": str(run_dir),
                "inputs": {} if inputs is None else inputs,
            }
        ),
        encoding="utf-8",
    )
    return request_path, run_dir


def read_manifest(run_dir: Path) -> dict:
    return json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))


def patch_success_path(monkeypatch, workflow_function):
    monkeypatch.setattr(worker, "SessionContext", lambda: object())
    monkeypatch.setattr(worker, "register_dataset", lambda ctx, path: None)
    monkeypatch.setattr(worker, "load_pack_modules", lambda path: [object()])
    monkeypatch.setattr(
        worker,
        "find_workflow",
        lambda modules, workflow_name: workflow_function,
    )
    monkeypatch.setattr(worker, "validate_artifacts", lambda result, run_dir: ["plan"])
    monkeypatch.setattr(
        worker,
        "materialize_artifacts",
        lambda plans: [{"name": "path_nodes", "path": "artifacts/path_nodes.parquet"}],
    )


def assert_failure(run_dir: Path, expected_kind: str, expected_type: str) -> None:
    manifest = read_manifest(run_dir)
    assert manifest["status"] == "failed"
    assert manifest["error"]["kind"] == expected_kind
    assert manifest["error"]["type"] == expected_type
    assert manifest["error"]["message"]
    assert "Traceback" in manifest["error"]["traceback"]


def test_request_contract_uses_request_parent_as_manifest_fallback(tmp_path):
    request_path = tmp_path / "request.json"
    request_path.write_text("{", encoding="utf-8")

    assert worker.run_request_file(request_path) == 1

    assert_failure(tmp_path, "request_contract", "JSONDecodeError")


@pytest.mark.parametrize(
    "payload",
    [
        [],
        {
            "packRoot": "pack",
            "workflow": "sample",
            "datasetPath": "dataset",
            "inputs": {},
        },
        {
            "packRoot": "pack",
            "workflow": "sample",
            "datasetPath": "dataset",
            "runDir": 42,
            "inputs": {},
        },
    ],
)
def test_request_contract_rejects_shape_and_invalid_run_dir(tmp_path, payload):
    request_path = tmp_path / "request.json"
    request_path.write_text(json.dumps(payload), encoding="utf-8")

    assert worker.run_request_file(request_path) == 1

    manifest = read_manifest(tmp_path)
    assert manifest["status"] == "failed"
    assert manifest["error"]["kind"] == "request_contract"


@pytest.mark.parametrize(
    "kind",
    [
        "session_creation",
        "dataset_registration",
        "pack_load",
        "workflow_selection",
        "input_contract",
        "workflow_execution",
        "return_contract",
        "materialization",
    ],
)
def test_worker_reports_stable_phase_kind(monkeypatch, tmp_path, kind):
    request_path, run_dir = write_request(tmp_path)

    def successful_workflow(kat):
        return {"path_nodes": object()}

    patch_success_path(monkeypatch, successful_workflow)
    expected_type = "RuntimeError"

    if kind == "session_creation":
        monkeypatch.setattr(
            worker,
            "SessionContext",
            lambda: (_ for _ in ()).throw(RuntimeError("session boom")),
        )
    elif kind == "dataset_registration":
        monkeypatch.setattr(
            worker,
            "register_dataset",
            lambda ctx, path: (_ for _ in ()).throw(ValueError("dataset boom")),
        )
        expected_type = "ValueError"
    elif kind == "pack_load":
        monkeypatch.setattr(
            worker,
            "load_pack_modules",
            lambda path: (_ for _ in ()).throw(ImportError("import boom")),
        )
        expected_type = "ImportError"
    elif kind == "workflow_selection":
        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: (_ for _ in ()).throw(KeyError("missing workflow")),
        )
        expected_type = "KeyError"
    elif kind == "input_contract":
        def requires_input(kat, required):
            raise AssertionError("workflow body must not run")

        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: requires_input,
        )
        expected_type = "TypeError"
    elif kind == "workflow_execution":
        def failing_workflow(kat):
            raise TypeError("body boom")

        monkeypatch.setattr(
            worker,
            "find_workflow",
            lambda modules, name: failing_workflow,
        )
        expected_type = "TypeError"
    elif kind == "return_contract":
        monkeypatch.setattr(
            worker,
            "validate_artifacts",
            lambda result, run_dir: (_ for _ in ()).throw(
                TypeError("return boom")
            ),
        )
        expected_type = "TypeError"
    elif kind == "materialization":
        monkeypatch.setattr(
            worker,
            "materialize_artifacts",
            lambda plans: (_ for _ in ()).throw(OSError("write boom")),
        )
        expected_type = "OSError"

    assert worker.run_request_file(request_path) == 1
    assert_failure(run_dir, kind, expected_type)
```

- [ ] **Step 2: Add a failing duplicate workflow selection test**

Append to the same file:

```python
def test_workflow_selection_rejects_duplicate_names(tmp_path):
    from kat_runtime.pack_loader import find_workflow, load_pack_modules

    pack = tmp_path / "pack"
    pack.mkdir()
    for module_name in ["one.py", "two.py"]:
        (pack / module_name).write_text(
            """
from kat import workflow

@workflow(title="Duplicate", description="Duplicate name")
def duplicate(kat):
    return {}
""",
            encoding="utf-8",
        )

    modules = load_pack_modules(pack)
    with pytest.raises(ValueError, match="workflow is ambiguous: duplicate"):
        find_workflow(modules, "duplicate")


def test_ambiguous_workflow_is_reported_as_workflow_selection(
    monkeypatch, tmp_path
):
    request_path, run_dir = write_request(tmp_path)

    def successful_workflow(kat):
        return {}

    patch_success_path(monkeypatch, successful_workflow)
    monkeypatch.setattr(
        worker,
        "find_workflow",
        lambda modules, name: (_ for _ in ()).throw(
            ValueError("workflow is ambiguous: sample")
        ),
    )

    assert worker.run_request_file(request_path) == 1
    assert_failure(run_dir, "workflow_selection", "ValueError")
```

- [ ] **Step 3: Run the worker tests and verify the current class-name behavior fails**

```powershell
python -m pytest python/tests/test_worker_error_contract.py -q -p no:cacheprovider --basetemp .pytest-tmp/worker-red
```

Expected: FAIL because request parsing is outside the try block, `run_request_file` is absent, errors use exception class names, `signature.bind()` is absent, and workflow selection returns the first duplicate.

- [ ] **Step 4: Separate Pack loading from unique workflow selection**

In `python/kat-python-runtime/kat_runtime/pack_loader.py`, replace `find_workflow` with:

```python
def find_workflow(modules: list[ModuleType], workflow_name: str):
    matches = []
    for module in modules:
        for value in _iter_module_capabilities(module):
            metadata = getattr(value, "__kat_capability__", None)
            if (
                metadata
                and metadata["kind"] == "workflow"
                and metadata["name"] == workflow_name
            ):
                matches.append(value)
    if not matches:
        raise KeyError(f"workflow not found: {workflow_name}")
    if len(matches) > 1:
        raise ValueError(f"workflow is ambiguous: {workflow_name}")
    return matches[0]
```

`discover_pack()` continues to call `load_pack_modules(pack_root)` directly.

- [ ] **Step 5: Implement request validation and explicit phase orchestration**

Replace `python/kat-python-runtime/kat_runtime/worker/run.py` with:

```python
from __future__ import annotations

import argparse
import inspect
import json
import sys
import traceback
from pathlib import Path
from typing import Any

from datafusion import SessionContext
from kat import Kat

from kat_runtime.artifacts import materialize_artifacts, validate_artifacts
from kat_runtime.dataset import register_dataset
from kat_runtime.manifest import now_iso, write_manifest
from kat_runtime.pack_loader import find_workflow, load_pack_modules


REQUIRED_STRING_FIELDS = ("packRoot", "workflow", "datasetPath", "runDir")


def _read_request(request_path: Path) -> dict[str, Any]:
    request = json.loads(request_path.read_text(encoding="utf-8"))
    if not isinstance(request, dict):
        raise TypeError("request must be a JSON object")
    for field in REQUIRED_STRING_FIELDS:
        value = request.get(field)
        if not isinstance(value, str) or not value:
            raise ValueError(f"request field {field} must be a non-empty string")
    inputs = request.get("inputs", {})
    if not isinstance(inputs, dict):
        raise TypeError("request field inputs must be an object")
    request["inputs"] = inputs
    return request


def _failure_manifest(
    request: dict[str, Any],
    *,
    kind: str,
    error: Exception,
    started_at: str,
) -> dict[str, Any]:
    return {
        "status": "failed",
        "packRoot": request.get("packRoot"),
        "workflow": request.get("workflow"),
        "datasetPath": request.get("datasetPath"),
        "error": {
            "kind": kind,
            "type": error.__class__.__name__,
            "message": str(error),
            "traceback": traceback.format_exc(),
        },
        "startedAt": started_at,
        "finishedAt": now_iso(),
    }


def run_request_file(request_path: Path) -> int:
    fallback_run_dir = request_path.parent
    run_dir = fallback_run_dir
    request: dict[str, Any] = {}
    started_at = now_iso()
    phase = "request_contract"

    try:
        request = _read_request(request_path)
        run_dir = Path(request["runDir"])

        phase = "session_creation"
        ctx = SessionContext()

        phase = "dataset_registration"
        register_dataset(ctx, Path(request["datasetPath"]))

        phase = "pack_load"
        modules = load_pack_modules(Path(request["packRoot"]))

        phase = "workflow_selection"
        workflow = find_workflow(modules, request["workflow"])
        kat = Kat(ctx=ctx, run_dir=str(run_dir), logger=None)

        phase = "input_contract"
        inspect.signature(workflow).bind(kat, **request["inputs"])

        phase = "workflow_execution"
        result = workflow(kat, **request["inputs"])

        phase = "return_contract"
        plans = validate_artifacts(result, run_dir)

        phase = "materialization"
        artifacts = materialize_artifacts(plans)

        manifest = {
            "status": "success",
            "packRoot": request["packRoot"],
            "workflow": request["workflow"],
            "datasetPath": request["datasetPath"],
            "artifacts": artifacts,
            "startedAt": started_at,
            "finishedAt": now_iso(),
        }
        write_manifest(run_dir, manifest)
        print(json.dumps({"status": "success", "artifacts": artifacts}, ensure_ascii=False))
        return 0
    except Exception as error:
        manifest = _failure_manifest(
            request,
            kind=phase,
            error=error,
            started_at=started_at,
        )
        write_error = None
        for manifest_dir in dict.fromkeys([run_dir, fallback_run_dir]):
            try:
                write_manifest(manifest_dir, manifest)
                write_error = None
                break
            except Exception as candidate_error:
                write_error = candidate_error
        if write_error is not None:
            print(f"failed to write failure manifest: {write_error}", file=sys.stderr)
        print(json.dumps({"status": "failed", "error": manifest["error"]}, ensure_ascii=False))
        return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    args = parser.parse_args()
    return run_request_file(Path(args.request))


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 6: Strengthen the success worker assertion to query the artifact**

At the end of `test_run_worker_materializes_returned_dataframes` in `python/tests/test_sdk_runtime_contract.py`, replace path-only assertions with:

```python
    artifact_path = run_dir / "artifacts" / "path_nodes.parquet"
    reader = datafusion.SessionContext()
    reader.register_parquet("path_nodes", str(artifact_path))

    assert manifest["status"] == "success"
    assert artifact_path.is_file()
    assert (run_dir / "artifacts" / "path_edges.parquet").is_file()
    assert reader.table("path_nodes").to_pydict() == {
        "itid": [405],
        "thread_name": ["main"],
    }
    assert "path_nodes" in result.stdout
```

- [ ] **Step 7: Run all Runtime/SDK contracts**

```powershell
python -m pytest python/tests/test_worker_error_contract.py python/tests/test_runtime_validation.py python/tests/test_sdk_runtime_contract.py -q -p no:cacheprovider --basetemp .pytest-tmp/worker-green
```

Expected: all tests pass; the parametrized error test reports all nine kinds, the body `TypeError` remains `workflow_execution`, and invalid return data creates no artifact.

- [ ] **Step 8: Commit the worker slice**

```powershell
git add python/kat-python-runtime/kat_runtime/pack_loader.py python/kat-python-runtime/kat_runtime/worker/run.py python/tests/test_worker_error_contract.py python/tests/test_sdk_runtime_contract.py
git commit -m "feat(runtime): 固定 Pack worker 错误阶段"
```

### Task 5: Stop CLI Source-Tree Injection

**Files:**
- Modify: `crates/kat-rs-cli/src/python_worker.rs`

**Interfaces:**
- Consumes: `KAT_RS_PYTHON` environment variable or `python` fallback.
- Produces: Python child processes that inherit the caller environment unchanged and never prepend repository SDK/Runtime directories.

- [ ] **Step 1: Change the unit test to require exact environment preservation**

Rename `run_discovery_uses_configured_python_and_sets_pythonpath` to `run_discovery_uses_configured_python_without_rewriting_pythonpath` and replace its `pythonpath` assertions with:

```rust
        assert_eq!(payload["pythonpath"], json!("existing-pythonpath"));
```

Keep the existing assertions for the configured executable and discovery arguments.

- [ ] **Step 2: Run the focused Rust test and observe injected source paths**

```powershell
cargo test -p kat-rs-cli python_worker::tests::run_discovery_uses_configured_python_without_rewriting_pythonpath -- --exact --nocapture
```

Expected: FAIL because the captured value currently starts with `python/kat-python-sdk` and `python/kat-python-runtime`.

- [ ] **Step 3: Remove only CLI-owned `PYTHONPATH` construction**

At the top of `python_worker.rs`, remove `ffi::OsString` from the imports. Replace `base_python_command()` and delete `pythonpath()`:

```rust
fn base_python_command() -> Command {
    let python = env::var_os("KAT_RS_PYTHON").unwrap_or_else(|| "python".into());
    Command::new(python)
}
```

Do not call `.env_remove("PYTHONPATH")` here: callers may intentionally supply a development path. Clean-wheel tests remove it at the acceptance boundary.

- [ ] **Step 4: Run all CLI unit and contract tests**

```powershell
cargo test -p kat-rs-cli --lib python_worker -- --nocapture
cargo test -p kat-rs-cli --test pack_run_contract -- --nocapture
```

Expected: both commands pass; the two existing ignored placeholders remain ignored until Task 7 replaces them.

- [ ] **Step 5: Commit the CLI environment slice**

```powershell
git add crates/kat-rs-cli/src/python_worker.rs
git commit -m "fix(cli): 停止注入 Python 源码路径"
```

### Task 6: Cross-Platform Clean-Wheel Verification

**Files:**
- Create: `python/tests/wheel_smoke.py`
- Create: `python/tests/verify_wheels.py`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/full-ci.yml`

**Interfaces:**
- Consumes: Task 1 wheels, Task 4 worker module, Task 5 non-injecting CLI boundary.
- Produces: `python python/tests/verify_wheels.py [--runtime-venv PATH]`, a clean Runtime interpreter optionally retained for Task 7, and Windows/Linux CI evidence.

- [ ] **Step 1: Create the in-venv smoke program**

Create `python/tests/wheel_smoke.py` with:

```python
from __future__ import annotations

import importlib.metadata
import json
import os
import subprocess
import sys
import sysconfig
from pathlib import Path

import datafusion
import kat
import kat_runtime
from datafusion import DataFrame, SessionContext
from kat import Kat


def assert_site_package(module) -> None:
    purelib = Path(sysconfig.get_paths()["purelib"]).resolve()
    module_path = Path(module.__file__).resolve()
    assert module_path.is_relative_to(purelib), (module.__name__, module_path, purelib)


def main() -> int:
    assert datafusion.__version__ == "54.0.0"
    assert importlib.metadata.version("kat-python-sdk") == "0.1.0"
    assert importlib.metadata.version("kat-python-runtime") == "0.1.0"
    runtime_requirements = set(
        importlib.metadata.requires("kat-python-runtime") or []
    )
    assert "kat-python-sdk==0.1.0" in runtime_requirements
    assert "datafusion==54.0.0" in runtime_requirements
    assert_site_package(kat)
    assert_site_package(kat_runtime)

    native = Kat(ctx=SessionContext()).sql(
        "select $value as value, $quoted as quoted",
        value=54,
        quoted="DataFusion's native parameter",
    )
    assert isinstance(native, DataFrame)
    assert native.to_pydict() == {
        "value": [54],
        "quoted": ["DataFusion's native parameter"],
    }

    root = Path(os.environ["KAT_WHEEL_SMOKE_ROOT"]).resolve()
    dataset = root / "dataset"
    tables = dataset / "tables"
    pack = root / "pack"
    run_dir = root / "run"
    tables.mkdir(parents=True)
    pack.mkdir(parents=True)

    SessionContext().sql(
        "select 405 as itid, 'main' as thread_name"
    ).write_parquet(tables / "thread.parquet")
    (dataset / "catalog.json").write_text(
        json.dumps(
            {
                "tables": [
                    {
                        "name": "thread",
                        "path": "tables/thread.parquet",
                        "kind": "source",
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    (pack / "pack.py").write_text(
        """
from kat import workflow

@workflow(title="Installed smoke", description="Use the installed runtime")
def installed_smoke(kat, min_itid: int):
    return {
        "path_nodes": kat.sql(
            "select itid, thread_name from thread where itid >= $min_itid",
            min_itid=min_itid,
        )
    }
""",
        encoding="utf-8",
    )

    child_env = os.environ.copy()
    child_env.pop("PYTHONPATH", None)
    child_env.pop("PYTHONHOME", None)
    discovery = subprocess.run(
        [
            sys.executable,
            "-I",
            "-m",
            "kat_runtime.worker.discovery",
            "--pack-root",
            str(pack),
        ],
        env=child_env,
        cwd=root,
        text=True,
        capture_output=True,
        check=True,
    )
    assert [item["name"] for item in json.loads(discovery.stdout)["workflows"]] == [
        "installed_smoke"
    ]

    request_path = root / "request.json"
    request_path.write_text(
        json.dumps(
            {
                "packRoot": str(pack),
                "workflow": "installed_smoke",
                "datasetPath": str(dataset),
                "runDir": str(run_dir),
                "inputs": {"min_itid": 400},
            }
        ),
        encoding="utf-8",
    )
    subprocess.run(
        [
            sys.executable,
            "-I",
            "-m",
            "kat_runtime.worker.run",
            "--request",
            str(request_path),
        ],
        env=child_env,
        cwd=root,
        text=True,
        capture_output=True,
        check=True,
    )

    manifest = json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))
    artifact = run_dir / manifest["artifacts"][0]["path"]
    reader = SessionContext()
    reader.register_parquet("path_nodes", str(artifact))
    assert manifest["status"] == "success"
    assert artifact.is_file()
    assert reader.table("path_nodes").to_pydict() == {
        "itid": [405],
        "thread_name": ["main"],
    }
    print("installed wheel smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Run the absent verifier once to establish the red boundary**

```powershell
python python/tests/verify_wheels.py
```

Expected: FAIL because `python/tests/verify_wheels.py` does not exist yet. This confirms Task 6 is adding an executable acceptance boundary rather than relying on source-path pytest.

- [ ] **Step 3: Create the cross-platform wheel builder and installer**

Create `python/tests/verify_wheels.py` with:

```python
from __future__ import annotations

import argparse
import importlib.util
import os
import subprocess
import sys
import tempfile
import venv
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_ROOT = REPO_ROOT / "python" / "kat-python-sdk"
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
SMOKE = REPO_ROOT / "python" / "tests" / "wheel_smoke.py"


def run(command: list[str], *, env=None, cwd=None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, env=env, cwd=cwd, check=True)


def venv_python(environment: Path) -> Path:
    if os.name == "nt":
        return environment / "Scripts" / "python.exe"
    return environment / "bin" / "python"


def create_venv(path: Path) -> Path:
    if path.exists():
        raise FileExistsError(f"clean venv target already exists: {path}")
    venv.EnvBuilder(with_pip=True, clear=False).create(path)
    return venv_python(path)


def clean_env() -> dict[str, str]:
    environment = os.environ.copy()
    environment.pop("PYTHONPATH", None)
    environment.pop("PYTHONHOME", None)
    return environment


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--runtime-venv",
        type=Path,
        help="retain the clean Runtime venv at this new path for CLI E2E",
    )
    args = parser.parse_args()
    if importlib.util.find_spec("build") is None:
        raise RuntimeError("install the build package with: python -m pip install build")

    with tempfile.TemporaryDirectory(
        prefix=".kat-wheel-verify-",
        dir=REPO_ROOT,
    ) as temporary:
        work = Path(temporary)
        dist = work / "dist"
        dist.mkdir()
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--outdir",
                str(dist),
                str(SDK_ROOT),
            ]
        )
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--outdir",
                str(dist),
                str(RUNTIME_ROOT),
            ]
        )
        sdk_wheel = next(dist.glob("kat_python_sdk-0.1.0-*.whl"))
        runtime_wheel = next(dist.glob("kat_python_runtime-0.1.0-*.whl"))

        sdk_python = create_venv(work / "sdk-only-venv")
        run(
            [
                str(sdk_python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--no-deps",
                str(sdk_wheel),
            ],
            env=clean_env(),
        )
        sdk_probe = """
import importlib.util
import pathlib
import sysconfig
import kat

purelib = pathlib.Path(sysconfig.get_paths()["purelib"]).resolve()
module_path = pathlib.Path(kat.__file__).resolve()
assert module_path.is_relative_to(purelib), (module_path, purelib)
assert (module_path.parent / "py.typed").is_file()
assert importlib.util.find_spec("datafusion") is None
"""
        run([str(sdk_python), "-I", "-c", sdk_probe], env=clean_env())

        runtime_venv = (
            args.runtime_venv.resolve()
            if args.runtime_venv is not None
            else work / "runtime-venv"
        )
        runtime_python = create_venv(runtime_venv)
        run(
            [
                str(runtime_python),
                "-m",
                "pip",
                "install",
                "--disable-pip-version-check",
                "--only-binary=:all:",
                str(sdk_wheel),
                str(runtime_wheel),
            ],
            env=clean_env(),
        )
        run([str(runtime_python), "-m", "pip", "check"], env=clean_env())

        smoke_root = work / "smoke"
        smoke_root.mkdir()
        smoke_env = clean_env()
        smoke_env["KAT_WHEEL_SMOKE_ROOT"] = str(smoke_root)
        run(
            [str(runtime_python), "-I", str(SMOKE)],
            env=smoke_env,
            cwd=work,
        )
        print(f"clean Runtime Python: {runtime_python}")
        print("wheel verification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 4: Run clean-wheel verification locally**

```powershell
Remove-Item Env:PYTHONPATH -ErrorAction SilentlyContinue
Remove-Item Env:PYTHONHOME -ErrorAction SilentlyContinue
python python/tests/verify_wheels.py
```

Expected final lines:

```text
installed wheel smoke passed
wheel verification passed
```

The SDK-only venv has no DataFusion installation, while the Runtime venv reports SDK 0.1.0 and DataFusion 54.0.0 from `site-packages`.

- [ ] **Step 5: Add the Ubuntu Python contract job to ordinary CI**

Append these steps after Rust `Check` in `.github/workflows/ci.yml`:

```yaml
      - name: Install Python
        uses: actions/setup-python@v6
        with:
          python-version: "3.13"

      - name: Install Python test dependencies
        run: python -m pip install datafusion==54.0.0 pytest

      - name: Test Python SDK, Runtime, and Pack contracts
        run: >-
          python -m pytest
          python/tests/test_sdk_runtime_contract.py
          python/tests/test_runtime_validation.py
          python/tests/test_worker_error_contract.py
          python/tests/test_openharmony_critical_path_pack.py
          -q -p no:cacheprovider --basetemp .pytest-tmp/ci
```

- [ ] **Step 6: Add Windows/Linux clean-wheel verification to Full CI**

In `.github/workflows/full-ci.yml`, add Python setup before the Rust test and wheel verification after it:

```yaml
      - name: Install Python
        uses: actions/setup-python@v6
        with:
          python-version: "3.13"

      - name: Install wheel build tooling
        run: python -m pip install build

      - name: Verify clean Python wheels
        run: python python/tests/verify_wheels.py
```

Keep the existing Windows/Linux matrix and `cargo test --locked` step unchanged.

- [ ] **Step 7: Commit clean-wheel verification and CI wiring**

```powershell
git add python/tests/wheel_smoke.py python/tests/verify_wheels.py .github/workflows/ci.yml .github/workflows/full-ci.yml
git commit -m "test(python): 验证 Windows Linux clean wheels"
```

### Task 7: Synthetic and Hash-Pinned Real CLI E2E

**Files:**
- Modify: `.gitignore`
- Modify: `crates/kat-rs-cli/tests/pack_run_contract.rs`
- Create: `crates/kat-rs-cli/tests/pack_run_e2e.rs`
- Create: `python/tests/verify_cli_e2e.py`
- Modify: `.github/workflows/full-ci.yml`

**Interfaces:**
- Consumes: real `CARGO_BIN_EXE_kat-rs`, clean Runtime interpreter retained by `verify_wheels.py --runtime-venv`, OpenHarmony example Pack.
- Produces: explicit `synthetic_sqlite_pack_run_e2e` CI test and `real_test_db_pack_run_e2e` local/release test, both entering through the actual CLI binary.

- [ ] **Step 1: Remove the two no-op E2E placeholders and protect the real database**

Delete these tests from `crates/kat-rs-cli/tests/pack_run_contract.rs`:

```rust
pack_run_real_python_smoke_is_available_for_manual_verification
local_test_db_e2e_contract_is_documented
```

Append these exact generated-file exclusions to `.gitignore`:

```gitignore
**/__pycache__/
*.py[cod]
/.pytest-tmp/
/.kat-wheel-verify-*/
/.python-runtime-venv/
/python/kat-python-sdk/build/
/python/kat-python-sdk/*.egg-info/
/python/kat-python-runtime/build/
/python/kat-python-runtime/*.egg-info/
/test/test.db
```

Verify the 61 MB database is ignored but not tracked:

```powershell
git check-ignore -v test/test.db
git ls-files --error-unmatch test/test.db
```

Expected: the first command names `.gitignore`; the second exits nonzero because the DB is not part of Git.

- [ ] **Step 2: Add the Rust E2E orchestration before its Python verifier exists**

Create `crates/kat-rs-cli/tests/pack_run_e2e.rs` with:

```rust
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;


fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("kat-rs-cli is under crates/")
        .to_path_buf()
}


fn clean_python() -> PathBuf {
    env::var_os("KAT_RS_PYTHON")
        .map(PathBuf::from)
        .expect("KAT_RS_PYTHON must point to a clean-venv Python executable")
}


fn assert_success(label: &str, output: Output) -> Output {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}


fn run_cli(python: &Path, args: Vec<OsString>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kat-rs"));
    command
        .args(args)
        .env("KAT_RS_PYTHON", python)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    assert_success("kat-rs", command.output().expect("start kat-rs"))
}


fn create_synthetic_sqlite(path: &Path) {
    let connection = Connection::open(path).expect("create synthetic SQLite");
    connection
        .execute_batch(
            r#"
CREATE TABLE thread_state(
  id INT, ts INT, dur INT, cpu INT, itid INT, tid INT, pid INT,
  state TEXT, arg_setid INT
);
INSERT INTO thread_state VALUES
  (1, 0, 400000, 0, 1, 10, 1000, 'S', NULL),
  (2, 400000, 100000, 0, 1, 10, 1000, 'R', NULL),
  (3, 0, 400000, 1, 2, 20, 1000, 'Running', NULL);

CREATE TABLE thread(
  id INT, itid INT, tid INT, name TEXT, start_ts INT, end_ts INT,
  ipid INT, is_main_thread INT, switch_count INT
);
INSERT INTO thread VALUES
  (1, 1, 10, 'main', 0, 500000, 100, 1, 2),
  (2, 2, 20, 'worker', 0, 500000, 100, 0, 1);

CREATE TABLE process(
  id INT, ipid INT, pid INT, name TEXT, start_ts INT, switch_count INT,
  thread_count INT, slice_count INT, mem_count INT
);
INSERT INTO process VALUES
  (1, 100, 1000, '.tencent.wechat', 0, 3, 2, 3, 0);

CREATE TABLE args(key INT, datatype INT, value INT, argset INT);
CREATE TABLE data_dict(id INT, data TEXT);

CREATE TABLE instant(
  ts INT, name TEXT, ref INT, wakeup_from INT, ref_type TEXT, value REAL
);
INSERT INTO instant VALUES
  (400000, 'sched_wakeup', 1, 2, 'itid', NULL);

CREATE TABLE sched_slice(
  id INT, ts INT, dur INT, ts_end INT, cpu INT, itid INT, ipid INT,
  end_state TEXT, priority INT, arg_setid INT
);
INSERT INTO sched_slice VALUES
  (1, 0, 400000, 400000, 1, 2, 100, 'R', 120, NULL);

CREATE TABLE callstack(
  id INT, ts INT, dur INT, callid INT, cat TEXT, name TEXT, depth INT,
  cookie INT, parent_id INT, argsetid INT, chainId TEXT, spanId TEXT,
  parentSpanId TEXT, flag TEXT, trace_level TEXT, trace_tag TEXT,
  custom_category TEXT, custom_args TEXT, child_callid INT
);
INSERT INTO callstack VALUES
  (1, 0, 400000, 2, '', 'worker_stack', 0, 0, NULL, NULL,
   NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);

CREATE TABLE frame_slice(
  id INT, ts INT, vsync INT, ipid INT, itid INT, callstack_id INT,
  dur INT, src TEXT, dst INT, type INT, type_desc TEXT, flag INT,
  depth INT, frame_no INT
);
INSERT INTO frame_slice VALUES
  (1, 0, 0, 100, 1, NULL, 500000, '', 0, 0, '', 0, 0, 1);
"#,
        )
        .expect("populate synthetic SQLite");
}


fn run_end_to_end(db: &Path, profile: &str) {
    let root = repo_root();
    let python = clean_python();
    let temporary = tempdir().expect("E2E tempdir");
    let dataset = temporary.path().join("dataset");
    let run_dir = temporary.path().join("run");
    let pack = root.join("packs").join("openharmony-critical-path");

    run_cli(
        &python,
        vec![
            "dataset".into(),
            "materialize".into(),
            "sqlite".into(),
            db.as_os_str().to_owned(),
            dataset.as_os_str().to_owned(),
        ],
    );
    let inspect_dataset = run_cli(
        &python,
        vec![
            "dataset".into(),
            "inspect".into(),
            dataset.as_os_str().to_owned(),
        ],
    );
    assert!(
        String::from_utf8_lossy(&inspect_dataset.stdout).contains("thread_state")
    );

    let inspect_pack = run_cli(
        &python,
        vec![
            "pack".into(),
            "inspect".into(),
            pack.as_os_str().to_owned(),
            "--json".into(),
        ],
    );
    let discovery: Value =
        serde_json::from_slice(&inspect_pack.stdout).expect("parse Pack discovery JSON");
    assert!(
        discovery["workflows"]
            .as_array()
            .expect("workflow array")
            .iter()
            .any(|item| item["name"] == "wechat_first_frame_critical_path")
    );

    run_cli(
        &python,
        vec![
            "pack".into(),
            "run".into(),
            pack.as_os_str().to_owned(),
            "wechat_first_frame_critical_path".into(),
            "--dataset".into(),
            dataset.as_os_str().to_owned(),
            "--run-dir".into(),
            run_dir.as_os_str().to_owned(),
        ],
    );

    let verifier = root.join("python").join("tests").join("verify_cli_e2e.py");
    let output = Command::new(&python)
        .arg("-I")
        .arg(verifier)
        .arg("--profile")
        .arg(profile)
        .arg("--db")
        .arg(db)
        .arg("--dataset")
        .arg(&dataset)
        .arg("--run-dir")
        .arg(&run_dir)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .output()
        .expect("start Python E2E verifier");
    let output = assert_success("Python E2E verifier", output);
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}


#[test]
#[ignore = "requires a clean-wheel KAT_RS_PYTHON; Full CI runs this explicitly"]
fn synthetic_sqlite_pack_run_e2e() {
    let temporary = tempdir().expect("synthetic fixture tempdir");
    let db = temporary.path().join("synthetic.db");
    create_synthetic_sqlite(&db);
    run_end_to_end(&db, "synthetic");
}


#[test]
#[ignore = "requires hash-pinned test.db and a clean-wheel KAT_RS_PYTHON"]
fn real_test_db_pack_run_e2e() {
    let db = env::var_os("KAT_RS_E2E_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("test").join("test.db"));
    assert!(
        db.is_file(),
        "real E2E database is missing; set KAT_RS_E2E_DB or provide {}",
        db.display(),
    );
    assert_eq!(
        fs::metadata(&db).expect("read test.db metadata").len(),
        61_009_920,
        "unexpected test.db length",
    );
    run_end_to_end(&db, "real");
}
```

- [ ] **Step 3: Prepare a clean Runtime venv and observe the missing verifier fail**

```powershell
$e2eRoot = Join-Path $env:TEMP 'kat-rs-datafusion54-e2e-red'
$venv = Join-Path $e2eRoot 'venv'
if (Test-Path -LiteralPath $e2eRoot) {
    $resolved = (Resolve-Path -LiteralPath $e2eRoot).Path
    $tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to remove outside TEMP: $resolved"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
python python/tests/verify_wheels.py --runtime-venv $venv
$env:KAT_RS_PYTHON = (Resolve-Path (Join-Path $venv 'Scripts/python.exe')).Path
Remove-Item Env:PYTHONPATH -ErrorAction SilentlyContinue
Remove-Item Env:PYTHONHOME -ErrorAction SilentlyContinue
cargo test -p kat-rs-cli --test pack_run_e2e synthetic_sqlite_pack_run_e2e -- --ignored --exact --nocapture
```

Expected: CLI materialize/inspect/run execute, then the test fails because `python/tests/verify_cli_e2e.py` does not exist.

- [ ] **Step 4: Add the deterministic DataFusion 54 artifact verifier**

Create `python/tests/verify_cli_e2e.py` with:

```python
from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import sysconfig
from pathlib import Path

import datafusion
import kat
import kat_runtime
import pyarrow as pa
from datafusion import SessionContext


EXPECTED_REAL_SHA256 = (
    "5f742a759c57bb05fe010e44a1f03aa042e4b7cf6ee53769fa55f7cfd6fe8829"
)
NODE_COLUMNS = [
    "node_id", "depth", "itid", "tid", "thread_name", "pid", "process_name",
    "window_start_ts", "window_end_ts", "segment_start_ts", "segment_end_ts",
    "dur", "state", "classification", "sched_cpu", "sched_priority",
    "callstack_name", "blocked_caller", "blocking_context_node_id",
    "inherited_blocked_caller", "confidence", "uncertainty",
    "termination_reason",
]
EDGE_COLUMNS = [
    "edge_id", "from_node_id", "to_node_id", "from_itid", "to_itid",
    "parent_depth", "child_depth", "wakeup_ts", "edge_type", "confidence",
    "reason",
]
NODE_INTEGER_COLUMNS = {
    "node_id", "depth", "itid", "tid", "pid", "window_start_ts",
    "window_end_ts", "segment_start_ts", "segment_end_ts", "dur",
    "sched_cpu", "sched_priority", "blocking_context_node_id",
}
EDGE_INTEGER_COLUMNS = {
    "edge_id", "from_node_id", "to_node_id", "from_itid", "to_itid",
    "parent_depth", "child_depth", "wakeup_ts",
}
REQUIRED_TABLES = {
    "thread_state", "thread", "process", "args", "data_dict", "instant",
    "sched_slice", "callstack", "frame_slice",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def scalar(ctx: SessionContext, sql: str) -> int:
    columns = ctx.sql(sql).to_pydict()
    assert len(columns) == 1, columns
    return next(iter(columns.values()))[0]


def assert_site_package(module) -> None:
    purelib = Path(sysconfig.get_paths()["purelib"]).resolve()
    module_path = Path(module.__file__).resolve()
    assert module_path.is_relative_to(purelib), (module.__name__, module_path, purelib)


def assert_artifact_schema(schema, columns: list[str], integer_columns: set[str]) -> None:
    assert schema.names == columns
    for name in columns:
        field = schema.field(name)
        assert field.nullable
        if name in integer_columns:
            assert pa.types.is_int64(field.type), (name, field.type)
        else:
            assert (
                pa.types.is_string(field.type)
                or pa.types.is_large_string(field.type)
                or pa.types.is_string_view(field.type)
            ), (name, field.type)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["synthetic", "real"], required=True)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    args = parser.parse_args()

    assert datafusion.__version__ == "54.0.0"
    assert importlib.metadata.version("kat-python-sdk") == "0.1.0"
    assert importlib.metadata.version("kat-python-runtime") == "0.1.0"
    assert_site_package(kat)
    assert_site_package(kat_runtime)
    if args.profile == "real":
        assert args.db.stat().st_size == 61_009_920
        assert sha256(args.db) == EXPECTED_REAL_SHA256

    catalog = json.loads((args.dataset / "catalog.json").read_text(encoding="utf-8"))
    tables = {item["name"]: item for item in catalog["tables"]}
    assert REQUIRED_TABLES <= tables.keys()
    manifest = json.loads((args.run_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["status"] == "success"
    artifacts = {item["name"]: item for item in manifest["artifacts"]}
    assert artifacts.keys() == {"path_nodes", "path_edges"}

    ctx = SessionContext()
    ctx.register_parquet(
        "path_nodes", str(args.run_dir / artifacts["path_nodes"]["path"])
    )
    ctx.register_parquet(
        "path_edges", str(args.run_dir / artifacts["path_edges"]["path"])
    )
    ctx.register_parquet(
        "instant", str(args.dataset / tables["instant"]["path"])
    )
    assert_artifact_schema(
        ctx.table("path_nodes").schema(), NODE_COLUMNS, NODE_INTEGER_COLUMNS
    )
    assert_artifact_schema(
        ctx.table("path_edges").schema(), EDGE_COLUMNS, EDGE_INTEGER_COLUMNS
    )

    expected = {
        "synthetic": {
            "nodes": 3,
            "edges": 2,
            "wakeup": 1,
            "sequence": 1,
            "itid": 1,
            "tid": 10,
            "pid": 1000,
            "thread_name": "main",
            "start": 0,
            "end": 500000,
        },
        "real": {
            "nodes": 333,
            "edges": 331,
            "wakeup": 61,
            "sequence": 270,
            "itid": 405,
            "tid": 15040,
            "pid": 15040,
            "thread_name": ".tencent.wechat",
            "start": 246306873000,
            "end": 246332420000,
        },
    }[args.profile]

    node_count = scalar(ctx, "select count(*) as value from path_nodes")
    distinct_nodes = scalar(
        ctx, "select count(distinct node_id) as value from path_nodes"
    )
    exact_target = scalar(
        ctx,
        f"""
        select count(*) as value
        from path_nodes
        where depth = 0
          and itid = {expected['itid']}
          and tid = {expected['tid']}
          and pid = {expected['pid']}
          and thread_name = '{expected['thread_name']}'
          and process_name = '.tencent.wechat'
          and window_start_ts = {expected['start']}
          and window_end_ts = {expected['end']}
        """,
    )
    bad_uncertainty = scalar(
        ctx,
        """
        select count(*) as value
        from path_nodes
        where termination_reason is not null and uncertainty is null
        """,
    )
    assert node_count == expected["nodes"] == distinct_nodes
    assert exact_target == 1
    assert bad_uncertainty == 0

    edge_count = scalar(ctx, "select count(*) as value from path_edges")
    distinct_edges = scalar(
        ctx, "select count(distinct edge_id) as value from path_edges"
    )
    wakeup_count = scalar(
        ctx, "select count(*) as value from path_edges where edge_type = 'wakeup'"
    )
    sequence_count = scalar(
        ctx, "select count(*) as value from path_edges where edge_type = 'sequence'"
    )
    invalid_edges = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges
        where edge_type is null
           or edge_type not in ('wakeup', 'sequence')
           or confidence is null
           or confidence <> 'fact'
           or (edge_type = 'wakeup' and (
                 wakeup_ts is null
                 or reason is null
                 or reason <> 'sched_wakeup'
                 or parent_depth is null
                 or child_depth is null
                 or child_depth <> parent_depth + 1
              ))
           or (edge_type = 'sequence' and (
                 wakeup_ts is not null
                 or reason is null
                 or reason <> 'thread_state_order'
                 or from_itid is null
                 or to_itid is null
                 or from_itid <> to_itid
                 or parent_depth is null
                 or child_depth is null
                 or parent_depth <> child_depth
              ))
        """,
    )
    bad_node_references = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges e
        left join path_nodes source on source.node_id = e.from_node_id
        left join path_nodes target on target.node_id = e.to_node_id
        where source.node_id is null
           or target.node_id is null
           or e.from_itid is null
           or e.to_itid is null
           or e.from_itid <> source.itid
           or e.to_itid <> target.itid
        """,
    )
    unmatched_wakeups = scalar(
        ctx,
        """
        select count(*) as value
        from path_edges e
        where e.edge_type = 'wakeup'
          and not exists (
            select 1
            from instant i
            where i.ref_type = 'itid'
              and i.name like 'sched_wakeup%'
              and i.wakeup_from = e.from_itid
              and i.ref = e.to_itid
              and i.ts = e.wakeup_ts
          )
        """,
    )
    assert edge_count == expected["edges"] == distinct_edges
    assert wakeup_count == expected["wakeup"]
    assert sequence_count == expected["sequence"]
    assert invalid_edges == 0
    assert bad_node_references == 0
    assert unmatched_wakeups == 0

    print(
        json.dumps(
            {
                "profile": args.profile,
                "datafusion": datafusion.__version__,
                "kat": kat.__file__,
                "kat_runtime": kat_runtime.__file__,
                "nodes": node_count,
                "edges": edge_count,
                "wakeup": wakeup_count,
            },
            ensure_ascii=False,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 5: Run the synthetic CLI E2E to green**

Reuse the `KAT_RS_PYTHON` prepared in Step 3:

```powershell
cargo test -p kat-rs-cli --test pack_run_e2e synthetic_sqlite_pack_run_e2e -- --ignored --exact --nocapture
```

Expected: PASS and verifier JSON with `nodes: 3`, `edges: 2`, `wakeup: 1`; the printed `kat` and `kat_runtime` paths are under the retained venv `site-packages`.

- [ ] **Step 6: Make Full CI retain the clean venv and execute synthetic CLI E2E**

Change the Task 6 wheel step in `.github/workflows/full-ci.yml` to:

```yaml
      - name: Verify clean Python wheels
        run: >-
          python python/tests/verify_wheels.py
          --runtime-venv .python-runtime-venv

      - name: Test synthetic SQLite through the real CLI
        env:
          KAT_RS_PYTHON: ${{ format('{0}/.python-runtime-venv/{1}', github.workspace, runner.os == 'Windows' && 'Scripts/python.exe' || 'bin/python') }}
          PYTHONPATH: ""
          PYTHONHOME: ""
        run: >-
          cargo test --locked -p kat-rs-cli
          --test pack_run_e2e synthetic_sqlite_pack_run_e2e
          -- --ignored --exact --nocapture
```

Expected: the matrix runs the same real CLI path on `ubuntu-latest` and `windows-latest` with only installed wheels visible to Python.

- [ ] **Step 7: Run the hash-pinned real `test.db` E2E**

With the same retained clean venv:

```powershell
$env:KAT_RS_E2E_DB = (Resolve-Path 'test/test.db').Path
cargo test -p kat-rs-cli --test pack_run_e2e real_test_db_pack_run_e2e -- --ignored --exact --nocapture
```

Expected: PASS after actual `dataset materialize sqlite`, `dataset inspect`, `pack inspect --json`, and `pack run`; verifier JSON reports DataFusion `54.0.0`, `nodes: 333`, `edges: 331`, `wakeup: 61`, and installed module paths. Missing/mismatched DB, any CLI nonzero exit, target mismatch, graph mismatch, or unmatched wakeup must fail.

- [ ] **Step 8: Commit both E2E gates**

```powershell
git add .gitignore crates/kat-rs-cli/tests/pack_run_contract.rs crates/kat-rs-cli/tests/pack_run_e2e.rs python/tests/verify_cli_e2e.py .github/workflows/full-ci.yml
git commit -m "test(cli): 覆盖合成与真实数据库 Pack E2E"
```

### Task 8: Full Regression and Delivery Evidence

**Files:**
- Verify only: all files changed by Tasks 1-7

**Interfaces:**
- Consumes: all task deliverables.
- Produces: review-ready command evidence with separate CI, real-data, and external-Pack statuses.

- [ ] **Step 1: Prove no legacy SQL or incorrect view API remains in the repository Pack**

```powershell
rg -n ":[A-Za-z_][A-Za-z0-9_]*|\.register_view\(" packs/openharmony-critical-path -g "*.py"
```

Expected: exit 1 with no output. `ctx.register_view(...)` would remain allowed if introduced later; this Pack currently needs no temporary view.

- [ ] **Step 2: Run the complete Python source-contract suite**

```powershell
python -m pytest python/tests/test_sdk_runtime_contract.py python/tests/test_runtime_validation.py python/tests/test_worker_error_contract.py python/tests/test_openharmony_critical_path_pack.py -q -p no:cacheprovider --basetemp .pytest-tmp/final
```

Expected: all tests pass with DataFusion 54.0.0. Any symlink test skipped on an unprivileged Windows host must be listed explicitly; the Ubuntu ordinary-CI run remains the authoritative symlink-escape check, while both Full CI platforms exercise normal canonical dataset registration through the CLI.

- [ ] **Step 3: Run Rust regression including compilation of both ignored E2E tests**

```powershell
cargo test --locked
```

Expected: all ordinary Rust tests pass; both `pack_run_e2e` tests compile and are reported ignored.

- [ ] **Step 4: Rebuild and verify wheels in a fresh disposable environment**

```powershell
Remove-Item Env:PYTHONPATH -ErrorAction SilentlyContinue
Remove-Item Env:PYTHONHOME -ErrorAction SilentlyContinue
python python/tests/verify_wheels.py
```

Expected: `pip check`, SDK-only import, discovery, worker, artifact query, module provenance, and DataFusion version assertions pass.

- [ ] **Step 5: Re-run the synthetic CLI gate in a newly retained venv**

```powershell
$finalRoot = Join-Path $env:TEMP 'kat-rs-datafusion54-final'
$finalVenv = Join-Path $finalRoot 'venv'
if (Test-Path -LiteralPath $finalRoot) {
    $resolved = (Resolve-Path -LiteralPath $finalRoot).Path
    $tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to remove outside TEMP: $resolved"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
python python/tests/verify_wheels.py --runtime-venv $finalVenv
$env:KAT_RS_PYTHON = (Resolve-Path (Join-Path $finalVenv 'Scripts/python.exe')).Path
cargo test --locked -p kat-rs-cli --test pack_run_e2e synthetic_sqlite_pack_run_e2e -- --ignored --exact --nocapture
```

Expected: PASS with the synthetic 3-node/2-edge/1-wakeup evidence.

- [ ] **Step 6: Re-run the real database gate and capture release evidence**

```powershell
$env:KAT_RS_E2E_DB = (Resolve-Path 'test/test.db').Path
Get-Item -LiteralPath $env:KAT_RS_E2E_DB | Select-Object FullName,Length
Get-FileHash -Algorithm SHA256 -LiteralPath $env:KAT_RS_E2E_DB
& $env:KAT_RS_PYTHON -I -c "import datafusion,kat,kat_runtime; print(datafusion.__version__); print(kat.__file__); print(kat_runtime.__file__)"
cargo test --locked -p kat-rs-cli --test pack_run_e2e real_test_db_pack_run_e2e -- --ignored --exact --nocapture
```

Expected: size `61009920`, hash `5F742A759C57BB05FE010E44A1F03AA042E4B7CF6EE53769FA55F7CFD6FE8829`, DataFusion `54.0.0`, venv `site-packages` paths, and a passing real E2E with 333 nodes/331 edges/61 matched wakeups.

- [ ] **Step 7: Check formatting, staged scope, and generated-file exclusion**

```powershell
cargo fmt --all -- --check
git diff --check -- .gitignore .github/workflows/ci.yml .github/workflows/full-ci.yml crates/kat-rs-cli/src/python_worker.rs crates/kat-rs-cli/tests/pack_run_contract.rs crates/kat-rs-cli/tests/pack_run_e2e.rs python/kat-python-sdk python/kat-python-runtime python/tests packs/openharmony-critical-path/facts
git status --short
git ls-files test/test.db
```

Expected: formatting and whitespace checks pass, `git ls-files` prints nothing for the DB, and status contains no wheel, venv, dataset, run, Parquet, database copy, or temp directory. Unrelated pre-existing user changes may remain but must not be staged in this delivery.

- [ ] **Step 8: Record the two independent completion statuses**

In the PR/release note, paste the actual outputs produced by Steps 2-6 under these exact headings:

```text
本仓 PR 门禁
- Python source contracts: paste the Step 2 command and pytest pass count
- Rust tests: paste the Step 3 command and cargo test summary
- Windows clean-wheel + synthetic CLI E2E: paste the Full CI Windows job URL and conclusion
- Linux clean-wheel + synthetic CLI E2E: paste the Full CI Linux job URL and conclusion
- Real test.db: record sha256=5f742a759c57bb05fe010e44a1f03aa042e4b7cf6ee53769fa55f7cfd6fe8829; datafusion=54.0.0; nodes=333; edges=331; wakeup=61; then paste the Step 6 result

整体 DataFusion 54 迁移状态
- External repository/worktree: copy the identity verbatim from the external task
- External commit SHA: copy the SHA verbatim from the external task
- External Pack name: copy the name verbatim from the external task
- Legacy :name / DataFrame.register_view scan: copy the scan output verbatim from the external task
- Exact external test command/result: copy the command and result verbatim from the external task
- Overall status: write `complete` only when all five external evidence lines are populated; otherwise write `pending (external evidence not supplied)` without blocking this repository PR
```

Do not invent external repository identity or test results.

## Acceptance Matrix

| Requirement | Automated gate | Manual/release gate |
| --- | --- | --- |
| SDK has no runtime DataFusion dependency | SDK-only venv in `verify_wheels.py` | wheel METADATA/module path evidence |
| Runtime exact dependency closure | Runtime venv `pip check` and metadata assertions | installed version output |
| `$name + param_values` | SDK real DataFusion test + Pack source scan | none |
| Dataset/catalog safety | `test_runtime_validation.py` on CI | none |
| Real DataFrame/full preflight | `test_runtime_validation.py` | none |
| Nine stable worker kinds | `test_worker_error_contract.py` | failure manifest inspection when diagnosing |
| CLI does not inject source paths | Rust unit test + module provenance | none |
| Windows/Linux installed-wheel path | Full CI matrix | none |
| Synthetic full CLI path | Full CI matrix `synthetic_sqlite_pack_run_e2e` | none |
| Real OpenHarmony trace path | not run in ordinary CI | hash-pinned `real_test_db_pack_run_e2e` |
| External Pack migration | outside this repository | independent evidence; overall status remains pending without it |
