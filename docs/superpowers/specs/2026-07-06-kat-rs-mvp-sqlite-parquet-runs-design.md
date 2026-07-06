# kat-rs MVP: SQLite Dataset Materialization and `/v1/runs`

## 1. 目标

本 MVP 要把 `resources/packs/openharmony-critical-task-extraction` 示例 pack 在 `test/test.db` 上跑通，并保持 kat-rs 的架构边界干净：

```text
SQLite test.db
  -> kat-rs-datasource SQLite materializer
  -> Parquet dataset + catalog.json
  -> kat-rs-daemon POST /v1/runs
  -> Pack Resource Model expansion
  -> DataFusion query / grep / branch / loop / summaries
  -> run-local working tables
  -> evidence / brief
```

成功标准不是离线脚本能跑，而是通过 REST 主链路完成：

- `POST /v1/datasets` 可以把 SQLite 输入物化为现有 Parquet catalog dataset。
- `POST /v1/runs` 可以接收 pack ref、dataset ref 和 inputs，并完成示例 pack run。
- `GET /v1/runs/{runId}/evidence` 和 `GET /v1/runs/{runId}/brief` 可以返回可审查结果。

## 2. 非目标

本 MVP 不实现以下内容：

- 不让 `/v1/runs` 直接读取 SQLite。
- 不在 daemon 内部实现 SQLite schema 读取、SQLite 查询或 SQLite 到 Arrow 的转换。
- 不实现完整 pack marketplace、远程安装、多用户鉴权或 TLS。
- 不实现完整 `sequence.*`、`interval.*`、`graph.*` operator 族。
- 不实现持久化 run store；run facts 第一版只在当前 daemon 进程内可信。
- 不把 derived artifact 发布回 dataset catalog。
- 不生成自然语言诊断报告；daemon 只输出 structured evidence 和 brief。

## 3. 分层职责

### 3.1 `kat-rs-datasource`

`kat-rs-datasource` 新增 SQLite dataset materialization 能力，职责包括：

- 打开 SQLite 文件。
- 读取 MVP 所需 OpenHarmony 表。
- 将 SQLite rows 转为 Arrow RecordBatch。
- 写入现有 Parquet dataset layout。
- 生成现有 `catalog.json`。
- 提供 `materialize_sqlite_dataset(input_path, dataset_path)` 这类库函数给 daemon 调用。

MVP 只物化示例 pack 需要的事实表：

```text
process
thread
callstack
thread_state
instant
```

`instant` 表必须显式保留 SQLite `rowid` 为查询列，因为当前 `resources/openharmony/query/candidate_path_edges.yaml` 使用 `instant.rowid` 作为 wakeup row ref。pack resource 的 `requires.tables.instant.columns` 也应补充 `rowid`，让契约与 SQL 一致。

### 3.2 `kat-rs-daemon`

`kat-rs-daemon` 只承担 REST 编排和 run runtime：

- `POST /v1/datasets` 新增 `source: "SQLITE"`，调用 datasource 的 SQLite materializer。
- `/v1/runs` 只接收已存在的 dataset ref，不接收 SQLite path。
- run 执行阶段通过 `TraceDatasource::from_dataset(...)` 注册 Parquet catalog 到 DataFusion。
- daemon 装载 Pack Resource Model、执行 pack expansion、调度 operator、维护 run facts、输出 evidence 和 brief。

daemon 不直接连接 SQLite，不复制 datasource 的导入逻辑。

## 4. REST Surface

### 4.1 Dataset Materialization

扩展现有 `POST /v1/datasets` request：

```json
{
  "dataset": {
    "name": "openharmony-test",
    "directory": "D:/work/kat_rs/0706/kat-rs/test/datasets"
  },
  "input": {
    "source": "SQLITE",
    "file": "D:/work/kat_rs/0706/kat-rs/test/test.db"
  }
}
```

响应复用现有 dataset response，成功后 dataset 目录包含 `catalog.json` 和 Parquet tables。daemon 不为 SQLite 单独新增导入 API。

### 4.2 Run Submission

新增 `POST /v1/runs`：

```json
{
  "packRef": "openharmony.critical_task_extraction",
  "dataset": {
    "name": "openharmony-test",
    "directory": "D:/work/kat_rs/0706/kat-rs/test/datasets"
  },
  "inputs": {
    "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
    "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
    "end_marker_pattern": "UIVsyncTask.*firstDrawFrame\\s*[:=]\\s*1"
  }
}
```

MVP 可以同步执行 run。响应包含：

- `runId`
- `status`
- `packRef`
- dataset 摘要
- step 数量
- evidence 数量
- brief section 数量

### 4.3 Run Read APIs

新增：

```text
GET /v1/runs/{runId}
GET /v1/runs/{runId}/evidence
GET /v1/runs/{runId}/brief
```

`GET /v1/runs/{runId}` 返回 run facts 摘要、step status、diagnostics 和 snapshot digest 摘要。`evidence` 和 `brief` 只返回结构化 JSON，不返回自然语言结论。

## 5. Pack Runtime MVP

### 5.1 Pack Loading and Expansion

run runtime 从 `resources/manifest.yaml` 定位 pack，然后读取：

```text
pack.yaml
flow.yaml
brief.yaml
imports 引用的 Resource Library YAML
```

expansion 输出 run-local execution snapshot，至少记录：

- pack identity
- dataset ref
- initial inputs
- flattened flow/control tree
- resource path
- resource digest
- inline SQL / grep / summaries spec
- context subscriptions / publications
- public output mapping
- brief spec snapshot

snapshot 用于审计和执行，不作为作者主界面。

### 5.2 Context Store

MVP 支持两种 carrier：

- `scalar`
- `interval`

普通顺序 scope 中，同名 slot 重复发布是结构性错误。loop scope 允许 `anchor_*` 这类状态 slot 在每轮迭代后推进为 latest version。每次 publication 记录 producing step、slot、carrier 和 value。

### 5.3 Operators

MVP operator 范围收敛为示例 pack 需要的最小集合。

`grep`：

- 根据 resource YAML 的 target table、columns、patterns、predicates、order_by 和 limit 执行。
- 输出 working table。
- 成功后按 `context.publishes` 从第一行输出发布 context slot。

`query`：

- 使用 context store 渲染 `{{ctx.slot}}` 和 `{{ctx.interval.start/end}}`。
- 通过 DataFusion SQL 执行。
- 注册输出 working table。
- 成功后按 `context.publishes` 发布 slot。
- 如果现有 resource SQL 存在 SQLite 与 DataFusion 的语法差异，只做最小兼容修正，并保持输出列和业务语义不变。

`branch`：

- MVP 只支持 `when.row_count.table equals 0`。
- 只执行 then/else 中的静态 step 列表。

`loop`：

- MVP 只支持 bounded loop。
- `max_iterations` 从 context slot 读取。
- 支持 table accumulator：`append_from` 每轮 body 输出表。
- 支持 `next_state.slot_prefix` 将下一轮 anchor rows 发布为最新 `anchor_*` context。

`summaries`：

- 支持 `row_count`、`max`、`sum`、`count_distinct`。
- 支持 refs 的列投影、排序和 `max_rows`。
- 输出 evidence records。

## 6. Run Facts

MVP run facts 保存在 daemon 进程内，进程结束后丢弃。run facts 至少包含：

- run identity
- status
- dataset ref
- execution snapshot
- step records
- working table registry
- context publications
- diagnostics
- evidence records
- brief records

step 使用简单 staging + commit 语义：operator 成功后才提交 working table、context publications 和 evidence。失败 step 的 staged 产物不进入后续 query context。

## 7. 验证

### 7.1 集成验证输入

使用 `resources/packs/openharmony-critical-task-extraction/examples/wechat_first_frame.yaml` 的 inputs：

```yaml
process_name_pattern: '(^|\.)tencent\.wechat$|^com\.tencent\.wechat$'
start_marker_pattern: 'HandleLaunchAbility.*com\.tencent\.wechat'
end_marker_pattern: 'UIVsyncTask.*firstDrawFrame\s*[:=]\s*1'
```

SQLite 输入为：

```text
test/test.db
```

### 7.2 预期结果

基于当前 `test/test.db` 的可验证结果：

- `process_matches = 1`
- `target_thread = 1`
- `target_window = 1`
- `window_dur = 609201000`
- `path_edges = 8`
- `path_steps = 8`
- `critical_tasks = 8`
- `total_ranked_duration_ns = 3544401000`

`critical_tasks` 第一名应满足：

- `task_type = work`
- `duration_ns = 481901000`
- `thread_name = .tencent.wechat`
- `label` 包含 `HandleLaunchAbility`
- `reason_code = lifecycle`
- `raw_refs = callstack:6387`

### 7.3 测试层级

建议测试分三层：

1. datasource contract test：SQLite fixture 或 `test/test.db` 物化后，`TraceDatasource::from_dataset` 可以用 DataFusion 查询五张表。
2. daemon API contract test：`POST /v1/datasets` 支持 `SQLITE`，响应仍复用 dataset envelope。
3. run integration test：通过 `/v1/runs` 执行示例 pack，并断言 evidence metrics 与 brief section。

## 8. 交付切片

首个实现 PR 应保持小步交付：

1. `kat-rs-datasource` 增加 SQLite 到 Parquet catalog materializer。
2. `kat-rs-daemon` 扩展 `POST /v1/datasets` 的 `SQLITE` 输入。
3. `kat-rs-daemon` 增加 `/v1/runs`、`/v1/runs/{runId}`、`/evidence`、`/brief`。
4. 支持示例 pack 所需的 expansion、context、operator 和 run facts。
5. 增加基于 `test/test.db` 的端到端验证。

如果第 4 步实现过大，可以拆成同一设计下的连续 PR，但第一个可宣称 MVP 成功的点必须能跑通 `/v1/runs` 并返回 evidence。
