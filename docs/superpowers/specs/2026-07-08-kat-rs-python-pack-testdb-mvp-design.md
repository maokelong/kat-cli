# kat-rs Python PACK test.db MVP 方案

## 1. 结论

本 MVP 的目标是让现有示例 `packs/critical-path` 按 Python-first PACK 设计，在本地 `test/test.db` 上跑通，并抽取微信首帧窗口的关键路径信息。

推荐切片：

```text
test/test.db
  -> kat-rs-datasource SQLite source
  -> Parquet dataset catalog
  -> TraceDatasource::from_dataset()
  -> minimal Python PACK runner
  -> packs/critical-path/workflows/extract.py
  -> run-local artifacts / summary
```

核心取舍：

- `test.db` 不直接暴露给 Python。SQLite 先物化为现有 `catalog.json + Parquet` dataset，再复用 DataFusion。
- `critical-path` workflow 继续保持通用能力，只接收 `root thread + start_ts + end_ts`，不写微信、首帧或 app launch 专用窗口逻辑。
- 微信首帧窗口只进入本地 smoke test 和示例输入，不进入通用 workflow。
- MVP 不新增完整 PACK REST/UI 管理面，只实现可测试的 runner 主链路，避免把半成品接口暴露为长期契约。
- 不把 `docs/critical-path.strategy.md` 和 `docs/superpowers/specs/2026-07-05-kat-rs-integrated-architecture-design.md` 纳入本次交付。

## 2. 背景

当前仓库已有三块基础：

- `kat-rs-datasource` 已有 Parquet dataset catalog，并可由 DataFusion 注册查询。
- `python/kat_sdk/kat` 已有极小 SDK 参考实现：`workflow`、`option`、`kat.query()`、`QueryResult.preview()`、`QueryResult.rows(max_rows)`。
- `packs/critical-path` 已有通用关键路径算法和 workflow，但还没有 Rust 侧 worker/runner 把它接到 DataFusion dataset。

`test/test.db` 是 SQLite 形态的 trace processor / SmartPerf 数据库，包含 `process`、`thread`、`thread_state`、`instant`、`callstack`、`frame_slice` 等表。微信进程在该库中为 `.tencent.wechat`，主线程 `itid=405`、`tid=15040`。

该数据库中 `app_startup` 和 `animation` 为空，因此首帧窗口不应依赖专用 app startup 表。MVP 用 trace 中已有事实确定一个验证窗口：

```text
root_itid = 405
start_ts  = 245615162000  # IconStart com.tencent.wechat
end_ts    = 246306873000  # 微信主线程首个 actual frame
```

## 3. 非目标

本 MVP 明确不做：

- 不实现 YAML flow engine。
- 不实现 PACK 上传、远端 registry 或权限模型。
- 不新增完整 `/v1/packs` REST 产品面。
- 不实现 run history 管理 UI。
- 不让 Python 直接连接 SQLite 或扫描全 trace。
- 不把微信首帧窗口定位逻辑写进 `packs/critical-path` 的通用 workflow。
- 不把 `test/test.db` 默认纳入 PR，除非后续明确决定提交该大文件。
- 不完成 bundled CPython 发布打包；runner 先通过可配置 Python executable 验证协议，后续再接发布包内 runtime。

## 4. 方案比较

### 4.1 推荐方案：SQLite source + dataset catalog + minimal runner

`kat-rs-datasource` 负责读取 SQLite，并把表和视图物化成 Parquet catalog。Runner 只面向 dataset 查询，不关心输入曾经是 hitrace、Langfuse 还是 SQLite。

优点：

- 保持 datasource 与 PACK 的边界清楚。
- DataFusion 仍是大表执行路径。
- `critical-path` pack 不需要 SQLite 特例。
- 可以用现有 `/v1/datasets` lifecycle 接入 `SQLITE` source，界面变化很小。

代价：

- 需要新增 SQLite 到 Arrow/Parquet 的类型映射。
- 需要一个最小 Python worker IPC 协议。

### 4.2 备选方案：Python 直接读 SQLite

让 workflow 通过 Python `sqlite3` 读取 `test.db`。

该方案最快，但破坏 Python-first 设计中“Python 不直接扫描大表、查询留给 DataFusion”的边界，因此不采用。

### 4.3 备选方案：一次性实现完整 PACK REST/runtime

直接实现 discovery/run/artifact/run history 的完整 REST 面。

该方案产品形态更完整，但会把首个 MVP 扩大到太多接口和长期契约，不利于 review，因此不采用。

## 5. 架构

```text
SQLite test.db
  -> materialize_sqlite_dataset()
  -> dataset/catalog.json + tables/*.parquet
  -> TraceDatasource::from_dataset()
  -> PackRunner
  -> Python worker
  -> kat SDK runtime channel
  -> TraceDatasource query_json()
  -> QueryResult handles
  -> returned QueryResult artifacts
  -> PackRunSummary
```

### 5.1 SQLite datasource

新增 `materialize_sqlite_dataset(sqlite_path, dataset_path)`。

职责：

- 打开 SQLite 文件。
- 从 `sqlite_master` 发现用户表和视图，排除 `sqlite_%` 系统对象。
- 通过 `PRAGMA table_info(<name>)` 获取列名和声明类型。
- 将 `INT/INTEGER` 映射为 Arrow `Int64`，`REAL/FLOAT/DOUBLE` 映射为 `Float64`，`TEXT` 和空类型映射为 `LargeUtf8`，`BLOB` 映射为 `LargeBinary`。
- 分批读取每个表或视图，写入 `DatasetWriter`。
- 生成现有 catalog 格式，所有物化对象使用 `kind: "source"`。

SQLite materializer 不理解 PACK、workflow、关键路径或微信语义。

### 5.2 daemon dataset 输入

扩展现有 dataset source enum：

```json
{
  "dataset": {
    "name": "wechat-testdb",
    "directory": "D:/tmp/kat-rs-datasets"
  },
  "input": {
    "source": "SQLITE",
    "file": "D:/work/kat_rs/0708/kat-rs/test/test.db"
  }
}
```

这只扩展已有 `/v1/datasets`，不新增独立 SQLite 专用接口。

### 5.3 minimal PackRunner

新增 Rust 侧 `PackRunner`，作为 daemon crate 内部能力，首版不暴露完整 REST 面。

输入：

- `dataset_path`
- `pack_root`
- `workflow_name`
- JSON-compatible `inputs`
- `run_dir`

输出：

- run id
- status
- logs
- returned artifact summaries
- failure traceback

MVP 的 artifact 可以保存为 run-local JSON 表和 metadata。它满足“只有 workflow 返回的 `QueryResult` 成为 artifact”的契约；Parquet artifact 文件格式可在下一切片补齐。

### 5.4 Python worker IPC

Worker 负责：

- 将 `python/kat_sdk` 和 PACK root 加入 `sys.path`。
- 递归扫描 PACK root 下 `**/*.py`。
- 根据文件相对路径推断 workflow name。
- 导入并定位目标 `@workflow` 函数。
- 绑定 `kat` SDK runtime channel。
- 执行 workflow。
- 校验返回值为 `dict[str, QueryResult]`。

Worker 与 Rust parent 通过 JSON line 协议交互：

```json
{"kind":"query","requestId":"r1","sql":"select ts, itid, state from thread_state where ts >= :start_ts","params":{"start_ts":245615162000}}
{"kind":"rows","requestId":"r2","queryId":"q1","maxRows":50000}
{"kind":"preview","requestId":"r3","queryId":"q2","limit":20}
{"kind":"complete","artifacts":{"path_nodes":"q7","path_edges":"q8"}}
```

Parent 返回：

```json
{"requestId":"r1","ok":true,"queryId":"q1"}
{"requestId":"r2","ok":true,"rows":[{"itid":405,"state":"S"}]}
```

### 5.5 QueryResult 执行模型

`kat.query()` 不立即收集全量结果，只在 Rust parent 中登记 SQL 和 params，返回 query id。

执行发生在三处：

- `preview(limit)`：用 `SELECT * FROM (<sql>) LIMIT limit` 读取小预览。
- `rows(max_rows)`：用 `LIMIT max_rows + 1` 做有界事实读取，超过上限报错。
- workflow 返回 artifact：对返回的 QueryResult 执行完整查询并保存 run-local artifact。

SQL 参数只接受 JSON scalar。Parent 将 `:name` 占位符渲染为 DataFusion SQL literal，并在渲染时跳过字符串字面量内部内容。

### 5.6 critical-path pack 兼容修正

当前 `WAKEUP_SQL` 使用 `rowid AS id`。SQLite 表转 Parquet 后不存在隐式 `rowid`，因此需要改成显式空 id：

```sql
SELECT
  CAST(NULL AS BIGINT) AS id,
  ts,
  ref AS target_itid,
  wakeup_from AS waker_itid,
  name
FROM instant
WHERE name IN ('sched_wakeup', 'sched_wakeup_new', 'sched_waking')
  AND ref_type = 'itid'
  AND wakeup_from IS NOT NULL
```

该修改让 pack 依赖显式列，不依赖 SQLite 隐含列，符合 DataFusion dataset 边界。

## 6. 数据流

1. 测试或用户调用 dataset create，source 为 `SQLITE`。
2. datasource 将 `test.db` 物化成 Parquet catalog。
3. runner 打开 dataset，创建 `TraceDatasource`。
4. runner 启动 Python worker。
5. worker 导入 `packs/critical-path/workflows/extract.py`。
6. workflow 调用 `kat.query()` 抽取 root/thread/state/wakeup/callstack 事实。
7. workflow 调用 `rows(max_rows)` 读取窗口内有界事实。
8. Python 算法构建关键路径结果。
9. workflow 用 `kat.query()` 执行由结果行生成的 `VALUES` SQL，产出 artifact QueryResult。
10. runner 只保存 workflow 返回的 QueryResult artifacts。
11. smoke test 断言 `path_nodes`、`path_edges`、`critical_path_evidence` 存在并包含微信主线程窗口信息。

## 7. 验证标准

### 7.1 datasource 验证

- SQLite 小 fixture 可以物化为 catalog。
- 表和视图都能被 DataFusion 查询。
- source SQLite 文件删除后，dataset 仍可查询。
- catalog 仍只包含现有 `tables` 字段和 `source` table kind。

### 7.2 daemon 验证

- `/v1/datasets` 接受 `source: "SQLITE"`。
- OpenAPI schema 包含 `SQLITE` source。
- 缺失 SQLite 文件返回现有 validation error。
- 不新增 `/v1/packs` 或 SQLite 专用 REST 路径。

### 7.3 runner 验证

- simple test pack 可以通过 `kat.query()`、`rows(max_rows)`、workflow return artifact 跑通。
- `rows(max_rows)` 超过 hard cap 报错。
- SQL 参数渲染支持 string/int/float/bool/null，并拒绝 object/array。
- workflow 抛异常时返回 traceback。

### 7.4 test.db smoke 验证

在本地存在 `test/test.db` 时，运行 ignored smoke test：

```text
cargo test -p kat-rs-daemon --test pack_runtime_contract -- --ignored
```

验收断言：

- SQLite materialization 成功。
- `process/thread/thread_state/instant/callstack/frame_slice` 可被 DataFusion 查询。
- `packs/critical-path` 的 `workflows.extract` 成功运行。
- artifact 至少包含 `target_window`、`path_nodes`、`path_edges`、`critical_path_evidence`。
- `target_window.root_itid = 405`。
- `path_nodes` 至少包含 `itid = 405` 的节点。
- `critical_path_evidence.node_count > 0`。

## 8. 风险与约束

- `test/test.db` 约 61MB，默认不进入 PR；smoke test 需要本地 fixture。
- SQLite 动态类型可能与声明类型不完全一致。MVP 按声明类型写 Arrow，遇到不可转换值时返回带表名、列名和 row index 的 validation error。
- 首版 worker 使用可配置 Python executable 做协议验证，不声明完成 bundled CPython 发布打包。
- artifact 首版可用 JSON table 保存；Parquet run-local artifact 是后续文件格式优化，不改变 workflow 返回契约。

## 9. 最小交付范围

本 MVP 的最小交付文件范围：

- SQLite materializer 与 datasource tests。
- daemon dataset source `SQLITE` enum/API tests。
- minimal Python worker 和 Rust PackRunner。
- `packs/critical-path` 的 DataFusion 兼容修正。
- 本地 `test/test.db` ignored smoke test。
- 本方案文档与对应实现计划。
