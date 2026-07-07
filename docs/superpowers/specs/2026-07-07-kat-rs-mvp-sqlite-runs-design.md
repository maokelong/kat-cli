# kat-rs MVP: SQLite dataset 与 runs 主链路设计

## 背景

`2026-07-05-kat-rs-integrated-architecture-design.md` 已把目标主链路收敛为：输入事实先进入 `kat-rs-datasource` 的 Arrow / Parquet / DataFusion dataset，再由 `kat-rs-daemon` 通过 REST 提交 pack run，最终产出可引用的 evidence。

本 MVP 只验证第一条可执行主链路：

```text
test/test.db
  -> kat-rs-datasource SQLite 五表 materializer
  -> Parquet dataset + catalog.json
  -> POST /v1/runs
  -> daemon 执行 packs/scheduling/app-launch-critical-path
  -> DataFusion working tables
  -> summaries evidence
  -> GET /v1/runs/{runId}/evidence
```

## 要解决的问题

1. 将 `test/test.db` 中 pack demo 需要的 SQLite 表转换成本地 Parquet dataset。
2. 复用现有 dataset catalog 与 DataFusion 查询路径，不在 daemon 复制 SQLite、Parquet 或 catalog 逻辑。
3. 新增干净的 `/v1/runs` REST 主入口，由调用方提交 `dataset`、`packRef` 和 `inputs`。
4. 用现有 `packs/scheduling/app-launch-critical-path/critical-task-extraction.yaml` 在 `test/test.db` 上端到端跑通，并产出结构化 evidence。

## 不做什么

1. 不把整个 SQLite 数据库全部转成 Parquet；本切片只转 pack demo 当前依赖的五张表。
2. 不实现完整 pack marketplace、权限、远程安装或任意插件机制。
3. 不实现异步 run job、取消、重试、进度流、跨进程恢复或长期 run store。
4. 不实现完整 `sequence.*`、`interval.*`、`graph.*` operator。
5. 不把 derived table 发布、report 生成或 LLM inference 放进本切片。
6. 不新增 CLI 业务命令；REST/OpenAPI 仍是唯一业务功能面。

## 方案选择

采用“SQLite 五表 materializer + 最小 pack runtime”。

备选方案：

1. **五表 materializer**：只转换 `process`、`thread`、`callstack`、`thread_state`、`instant`。实现面最小，能直接验证当前 pack demo。本 MVP 选择它。
2. **整库 SQLite materializer**：后续更通用，但会提前处理大量当前 pack 不需要的空表、宽表、类型和性能问题。
3. **直接查询 SQLite**：短期少写 materializer，但绕过 Parquet catalog，不能验证目标架构的数据底座。

## SQLite dataset materializer

`kat-rs-datasource` 新增 `materialize_sqlite_pack_demo_dataset(sqlite_path, dataset_path)`。它只读取以下源表：

```text
process
thread
callstack
thread_state
instant
```

规则：

1. 缺少任一表时 materialize 失败。
2. 每张表按 SQLite 当前列完整写入 Parquet，避免 daemon 为 pack SQL 维护列投影。
3. `instant` 额外写出显式 `rowid` 列，因为当前 pack SQL 使用 `instant.rowid` 作为 wakeup evidence ref。
4. SQLite 类型映射收敛为 `INTEGER -> Int64`、`REAL -> Float64`、`TEXT -> Utf8`、`BLOB -> Binary`、`NULL -> Utf8 nullable`。遇到同一列混合类型时，以读取到的 SQLite value 转成目标 Arrow 列；不能安全转换时 materialize 失败。
5. 输出仍复用现有 layout：

```text
<dataset>/
  catalog.json
  tables/
    sqlite.process.parquet
    sqlite.thread.parquet
    sqlite.callstack.parquet
    sqlite.thread_state.parquet
    sqlite.instant.parquet
```

catalog entry 继续写 `kind: "source"`，逻辑表名保持 `process`、`thread`、`callstack`、`thread_state`、`instant`，让 pack SQL 不需要知道物理来源。

## Dataset REST

现有 `POST /v1/datasets` 增加一个 source 变体：

```json
{
  "dataset": {
    "name": "test-db",
    "directory": "D:/work/kat_rs/0707/kat-rs/.kat-rs/datasets"
  },
  "input": {
    "source": "SQLITE",
    "file": "D:/work/kat_rs/0707/kat-rs/test/test.db"
  }
}
```

daemon 只负责 DTO、路径校验、并发限制和错误映射。SQLite 读取、Arrow batch 构建、Parquet 写入、catalog 校验仍由 `kat-rs-datasource` 拥有。

## Runs REST

新增最小 run 资源：

```text
POST /v1/runs
GET  /v1/runs/{runId}
GET  /v1/runs/{runId}/evidence
```

`POST /v1/runs` 请求：

```json
{
  "dataset": {
    "name": "test-db",
    "directory": "D:/work/kat_rs/0707/kat-rs/.kat-rs/datasets"
  },
  "packRef": "scheduling/app-launch-critical-path/critical-task-extraction",
  "inputs": {
    "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
    "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
    "end_marker_pattern": "UIVsyncTask.*firstDrawFrame:1"
  }
}
```

MVP 使用同步执行：HTTP 返回时 run 已经是 `SUCCEEDED` 或 `FAILED`。成功响应包含 `runId`、`status`、`dataset`、`packRef`、`outputs` 和 evidence 数量摘要。`GET /v1/runs/{runId}` 返回同一份轻量状态；`GET /v1/runs/{runId}/evidence` 返回 evidence records。

run 状态先保存在 daemon 进程内 `RunStore`。进程退出后旧 run 不承诺可恢复，符合本地可重建产物的当前边界。

## 最小 pack runtime

daemon 新增 run service 与 pack runtime 模块。MVP 只支持当前 pack demo 使用的资源形态：

```text
kind: flow
kind: query
kind: summaries
run
if_empty
repeat_until(empty + max_iterations)
outputs.set
outputs.append
```

资源解析规则：

1. `packRef` 只接受 manifest 可发现的本地 pack，MVP 验证目标为 `scheduling/app-launch-critical-path/critical-task-extraction`。
2. `common.*` 从 `packs/common/...` 解析。
3. `local.*` 从当前 pack 目录的 `local/...` 解析。
4. expansion 记录每个资源的 resolved path、digest 和 expanded content，形成本次 run 的 execution snapshot。
5. daemon 后续执行只读 snapshot 中显式引用的资源，不重新扫描目录。

query operator：

1. datasource 提供 dataset DataFusion context 构建能力；daemon 不直接读取 `catalog.json`。
2. 每个 query YAML 的 SQL 经受控模板替换后交给 DataFusion。
3. scalar input 只支持 string、integer、number、boolean。模板替换时对 string 做 SQL literal escaping；table input 通过已注册 working table 名称传递。
4. query 输出 RecordBatch 注册成 run-local working table。
5. `set` 替换变量当前表；`append` 将本次输出追加到累计表。

summaries operator：

MVP 只实现当前 `critical_task_evidence.yaml` 用到的 evidence 能力：

1. metrics: `row_count`、`max`、`sum`、`count_distinct`。
2. refs: `table`、可选 `columns`、`order_by`、`max_rows`。
3. evidence record 包含 `id`、`fact`、`metrics`、`refs`、`producingStep`。

## 错误处理

结构性错误 fail-fast，run 进入 `FAILED`：

1. SQLite 五表缺失或列转换失败。
2. dataset 不存在、catalog 非法或 Parquet 无法注册。
3. packRef 不存在、资源坐标无法解析、YAML schema 不符合 MVP 支持范围。
4. run input 缺失、类型不匹配或引用未定义变量。
5. SQL parse、planning 或 execution 失败。
6. summaries 引用不存在的 working table 或 column。

数据为空不自动失败。空匹配、空候选边、循环到达 `max_iterations` 等情况由 `if_empty`、`repeat_until` 和 summaries evidence 表达。

## 验证

自动化验证：

1. datasource contract：用 `test/test.db` materialize 后，catalog 只包含五张 source 表，`instant` 包含 `rowid`，`TraceDatasource::from_dataset` 可查询 `.tencent.wechat`。
2. API contract：`POST /v1/datasets` 支持 `SQLITE` source，并能创建可查询 dataset。
3. run contract：`POST /v1/runs` 使用示例输入在 `test.db` dataset 上成功，返回 `SUCCEEDED`。
4. evidence contract：`GET /v1/runs/{runId}/evidence` 至少包含 `target_window_shape` 和 `critical_task_shape`，并带有 metrics 与 refs。
5. OpenAPI contract：包含 `SQLITE` source、`POST /v1/runs`、`GET /v1/runs/{runId}`、`GET /v1/runs/{runId}/evidence`。

提交前验证：

```text
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

## 最小交付顺序

1. 在 `kat-rs-datasource` 增加 SQLite 五表 materializer 和 dataset contract tests。
2. 在 daemon dataset DTO / service / OpenAPI 中接入 `SQLITE` source。
3. 增加 run DTO、routes、进程内 `RunStore`。
4. 增加 pack expansion、query execution 和 summaries evidence 的 MVP runtime。
5. 用 `test/test.db` 和现有 pack demo 增加端到端 API contract。
