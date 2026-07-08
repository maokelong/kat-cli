# kat-rs Python PACK test.db MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让示例 `packs/critical-path` 通过 Python-first PACK runtime MVP 在本地 `test/test.db` 上跑通，并抽取微信首帧窗口关键路径 artifact。

**Architecture:** SQLite 先在 `kat-rs-datasource` 中物化为现有 Parquet dataset catalog，然后由 DataFusion 查询。Rust 侧新增 minimal PackRunner 和 Python worker IPC，Python pack 只通过 `kat.query()`、`QueryResult.rows(max_rows)` 和 workflow 返回值与 runtime 交互。

**Tech Stack:** Rust 2024、DataFusion 53.1、Arrow/Parquet 58、rusqlite、Axum API tests、Python 3 标准库、kat Python SDK、unittest/cargo test。

## Global Constraints

- `critical-path` workflow 必须保持通用，只接收 root thread 与时间窗口，不写微信、首帧或 app launch 专用逻辑。
- Python 不直接读取 SQLite，不扫描全 trace；大表过滤和查询通过 DataFusion。
- SQLite 输入必须先转为现有 `catalog.json + Parquet` dataset，再被 `TraceDatasource::from_dataset()` 注册。
- MVP 不新增完整 `/v1/packs` REST 产品面，不引入 YAML flow engine 或 operator registry。
- 只有 workflow 返回的 `QueryResult` 会成为 run-local artifact。
- `test/test.db` 默认作为本地验证 fixture，不纳入 PR，除非后续明确要求提交。
- 本计划不修改 `docs/critical-path.strategy.md` 和 `docs/superpowers/specs/2026-07-05-kat-rs-integrated-architecture-design.md`。

---

## File Structure

- Modify: `Cargo.toml`
  - Add workspace dependency `rusqlite = { version = "0.37", features = ["bundled"] }`.
- Modify: `crates/kat-rs-datasource/Cargo.toml`
  - Consume workspace `rusqlite`.
- Modify: `crates/kat-rs-datasource/src/formats/mod.rs`
  - Export `sqlite`.
- Create: `crates/kat-rs-datasource/src/formats/sqlite/mod.rs`
  - Discover SQLite tables/views and convert rows to Arrow batches.
- Modify: `crates/kat-rs-datasource/src/materializer.rs`
  - Add `materialize_sqlite_dataset()`.
- Modify: `crates/kat-rs-datasource/src/lib.rs`
  - Export `materialize_sqlite_dataset`.
- Modify: `crates/kat-rs-datasource/tests/dataset_contract.rs`
  - Add SQLite materialization tests.
- Modify: `crates/kat-rs-daemon/src/api.rs`
  - Add `SQLITE` source variant to `DatasetSourceInput`.
- Modify: `crates/kat-rs-daemon/src/dataset_service.rs`
  - Route `SQLITE` input to `materialize_sqlite_dataset()`.
- Modify: `crates/kat-rs-daemon/src/openapi.rs`
  - Schema updates come from the enum; keep path list unchanged.
- Modify: `crates/kat-rs-daemon/tests/api_contract.rs`
  - Add dataset lifecycle/API tests for `SQLITE`.
- Create: `crates/kat-rs-daemon/src/pack_runtime.rs`
  - Implement `PackRunner`, query registry, SQL parameter rendering, artifact summary.
- Modify: `crates/kat-rs-daemon/src/lib.rs`
  - Export `pack_runtime` for integration tests.
- Create: `python/kat_worker/kat_worker.py`
  - Implement Python worker protocol and workflow execution.
- Create: `crates/kat-rs-daemon/tests/pack_runtime_contract.rs`
  - Test minimal pack runner and ignored `test/test.db` smoke.
- Modify: `packs/critical-path/lib/facts.py`
  - Remove SQLite `rowid` dependency from `WAKEUP_SQL`.

---

### Task 1: SQLite -> Parquet Dataset Materializer

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/kat-rs-datasource/Cargo.toml`
- Modify: `crates/kat-rs-datasource/src/formats/mod.rs`
- Create: `crates/kat-rs-datasource/src/formats/sqlite/mod.rs`
- Modify: `crates/kat-rs-datasource/src/materializer.rs`
- Modify: `crates/kat-rs-datasource/src/lib.rs`
- Test: `crates/kat-rs-datasource/tests/dataset_contract.rs`

**Interfaces:**
- Produces: `pub async fn materialize_sqlite_dataset(path: impl AsRef<Path>, dataset_path: impl AsRef<Path>) -> anyhow::Result<()>`
- Produces: `pub(crate) fn sqlite_objects(conn: &rusqlite::Connection) -> anyhow::Result<Vec<SqliteObject>>`
- Produces: `pub(crate) fn stream_sqlite_object(conn: &rusqlite::Connection, object: &SqliteObject, writer: &mut DatasetTableWriter) -> anyhow::Result<()>`

- [ ] **Step 1: Write failing SQLite materialization test**

Add this test to `crates/kat-rs-datasource/tests/dataset_contract.rs`:

```rust
#[tokio::test]
async fn sqlite_dataset_materializes_tables_and_views_to_queryable_catalog() {
    let dir = tempdir().expect("tempdir");
    let sqlite_path = dir.path().join("trace.db");
    let dataset_path = dir.path().join("dataset");
    create_sqlite_fixture(&sqlite_path);

    kat_rs_datasource::materialize_sqlite_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json("select id, name from thread_names order by id")
        .await
        .expect("view query works");

    assert_eq!(
        rows,
        json!([
            { "id": 1, "name": "main" },
            { "id": 2, "name": "worker" }
        ])
    );

    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dataset_path.join("catalog.json")).expect("catalog is read"),
    )
    .expect("catalog parses");
    assert_eq!(catalog["tables"][0]["kind"], "source");
}

fn create_sqlite_fixture(path: &Path) {
    let conn = rusqlite::Connection::open(path).expect("sqlite opens");
    conn.execute_batch(
        "
        create table thread(id int, name text, cpu real, payload blob);
        insert into thread values (1, 'main', 1.5, x'0102');
        insert into thread values (2, 'worker', 2.5, null);
        create view thread_names as select id, name from thread;
        ",
    )
    .expect("fixture schema is created");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p kat-rs-datasource sqlite_dataset_materializes_tables_and_views_to_queryable_catalog
```

Expected: compile fails because `materialize_sqlite_dataset` and `rusqlite` are not defined.

- [ ] **Step 3: Add dependency declarations**

In root `Cargo.toml`, add:

```toml
rusqlite = { version = "0.37", features = ["bundled"] }
```

In `crates/kat-rs-datasource/Cargo.toml`, add:

```toml
rusqlite.workspace = true
```

- [ ] **Step 4: Add SQLite format module skeleton**

Create `crates/kat-rs-datasource/src/formats/sqlite/mod.rs` with these public crate-local types and signatures:

```rust
use std::path::Path;

use anyhow::Result;
use arrow_array::RecordBatch;
use rusqlite::Connection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteObject {
    pub(crate) name: String,
    pub(crate) kind: SqliteObjectKind,
    pub(crate) columns: Vec<SqliteColumn>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SqliteObjectKind {
    Table,
    View,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SqliteColumn {
    pub(crate) name: String,
    pub(crate) declared_type: String,
}

pub(crate) fn open(path: &Path) -> Result<Connection> {
    Ok(Connection::open(path)?)
}

pub(crate) fn objects(conn: &Connection) -> Result<Vec<SqliteObject>> {
    let mut stmt = conn.prepare(
        "select name, type from sqlite_master \
         where type in ('table', 'view') and name not like 'sqlite_%' \
         order by name",
    )?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let kind_text: String = row.get(1)?;
        let kind = match kind_text.as_str() {
            "table" => SqliteObjectKind::Table,
            "view" => SqliteObjectKind::View,
            _ => unreachable!("sqlite_master query filters object kind"),
        };
        Ok((name, kind))
    })?;

    let mut objects = Vec::new();
    for row in rows {
        let (name, kind) = row?;
        let columns = columns(conn, &name)?;
        objects.push(SqliteObject { name, kind, columns });
    }
    Ok(objects)
}

pub(crate) fn object_batches(conn: &Connection, object: &SqliteObject, batch_size: usize) -> Result<Vec<RecordBatch>> {
    let query = select_all_sql(object);
    let mut stmt = conn.prepare(&query)?;
    let mut rows = stmt.query([])?;
    let mut batches = Vec::new();
    let mut builders = SqliteBatchBuilders::new(object)?;

    while let Some(row) = rows.next()? {
        builders.append_row(row)?;
        if builders.len() >= batch_size {
            batches.push(builders.finish()?);
            builders = SqliteBatchBuilders::new(object)?;
        }
    }

    if builders.len() > 0 {
        batches.push(builders.finish()?);
    }
    Ok(batches)
}
```

- [ ] **Step 5: Implement object discovery**

Implement `objects()` so it executes:

```sql
select name, type
from sqlite_master
where type in ('table', 'view')
  and name not like 'sqlite_%'
order by name
```

For each object, run:

```sql
pragma table_info("<escaped object name>")
```

Use double-quote escaping for identifiers by replacing `"` with `""`.

- [ ] **Step 6: Implement Arrow conversion**

Implement declared type mapping:

```rust
fn arrow_type(declared: &str) -> arrow_schema::DataType {
    let upper = declared.trim().to_ascii_uppercase();
    if upper.contains("INT") {
        arrow_schema::DataType::Int64
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        arrow_schema::DataType::Float64
    } else if upper.contains("BLOB") {
        arrow_schema::DataType::LargeBinary
    } else {
        arrow_schema::DataType::LargeUtf8
    }
}
```

Build batches by selecting all columns from the object:

```sql
select "col1", "col2"
from "object"
```

Use nullable Arrow fields for all SQLite columns. Convert incompatible values into a validation error containing object name and column name.

- [ ] **Step 7: Wire materializer**

In `crates/kat-rs-datasource/src/materializer.rs`, add:

```rust
const SQLITE_DATASET_BATCH_ROWS: usize = 8192;

pub async fn materialize_sqlite_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();
    let mut writer = DatasetWriter::create(dataset_path)?;
    write_sqlite_tables(&mut writer, path)
        .with_context(|| format!("failed to write SQLite dataset tables: {}", dataset_path.display()))?;
    writer.finish().await
}
```

For each `SqliteObject`, start a table with file name `sqlite.<object>.parquet`, write all batches, then `add_table(table_writer.finish()?)`.

- [ ] **Step 8: Export module and function**

In `crates/kat-rs-datasource/src/formats/mod.rs`:

```rust
pub(crate) mod sqlite;
```

In `crates/kat-rs-datasource/src/lib.rs`:

```rust
pub use materializer::{
    materialize_hitrace_dataset,
    materialize_langfuse_legacy_dataset,
    materialize_sqlite_dataset,
};
```

- [ ] **Step 9: Verify task passes**

Run:

```powershell
cargo test -p kat-rs-datasource sqlite_dataset_materializes_tables_and_views_to_queryable_catalog
```

Expected: PASS.

- [ ] **Step 10: Run datasource regression tests**

Run:

```powershell
cargo test -p kat-rs-datasource dataset_contract
```

Expected: all dataset contract tests pass.

---

### Task 2: Daemon Dataset Source SQLITE

**Files:**
- Modify: `crates/kat-rs-daemon/src/api.rs`
- Modify: `crates/kat-rs-daemon/src/dataset_service.rs`
- Modify: `crates/kat-rs-daemon/tests/api_contract.rs`

**Interfaces:**
- Consumes: `kat_rs_datasource::materialize_sqlite_dataset(path, dataset_path)`
- Produces: `DatasetSourceInput::Sqlite { file: String }`
- Produces: `DatasetLoad::Sqlite { path: PathBuf }`

- [ ] **Step 1: Write failing API contract test**

Add a test to `crates/kat-rs-daemon/tests/api_contract.rs`:

```rust
#[tokio::test]
async fn dataset_create_materializes_sqlite_fixture_and_can_query_without_source() {
    let fixture = SqliteFixture::new();
    let datasets_dir = tempdir().expect("datasets tempdir is created");
    let datasets_root = datasets_dir.path().join("datasets");
    let dataset_name = "sqlite-fixture";
    let dataset_path = datasets_root.join(dataset_name);
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create = request_json(
        app.clone(),
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "SQLITE",
                "file": fixture.path(),
            }
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);

    fs::remove_file(fixture.path_buf()).expect("sqlite source is removed");

    let query = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "sql": "select id, name from thread_names order by id"
        })),
    )
    .await;

    assert_eq!(query.status, StatusCode::OK, "{:?}", query.body);
    assert_eq!(query.body["meta"]["dataset"]["path"], dataset_path.to_string_lossy().as_ref());
    assert_eq!(
        query.body["data"],
        json!([
            { "id": 1, "name": "main" },
            { "id": 2, "name": "worker" }
        ])
    );
}

struct SqliteFixture {
    _dir: TempDir,
    path: PathBuf,
}

impl SqliteFixture {
    fn new() -> Self {
        let dir = tempdir().expect("sqlite fixture tempdir is created");
        let path = dir.path().join("trace.db");
        let conn = rusqlite::Connection::open(&path).expect("sqlite opens");
        conn.execute_batch(
            "
            create table thread(id int, name text);
            insert into thread values (1, 'main');
            insert into thread values (2, 'worker');
            create view thread_names as select id, name from thread;
            ",
        )
        .expect("sqlite fixture is created");
        Self { _dir: dir, path }
    }

    fn path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    fn path_buf(&self) -> &Path {
        &self.path
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p kat-rs-daemon dataset_create_materializes_sqlite_fixture_and_can_query_without_source
```

Expected: request fails or compile fails because `SQLITE` is not in `DatasetSourceInput`.

- [ ] **Step 3: Add API enum variant**

In `crates/kat-rs-daemon/src/api.rs`, extend `DatasetSourceInput`:

```rust
#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "source", deny_unknown_fields)]
pub enum DatasetSourceInput {
    #[serde(rename = "HITRACE")]
    Hitrace { file: String },
    #[serde(rename = "LANGFUSE_LEGACY")]
    LangfuseLegacy {
        #[serde(rename = "observationsFile")]
        observations_file: String,
        #[serde(rename = "tracesFile")]
        traces_file: String,
    },
    #[serde(rename = "SQLITE")]
    Sqlite { file: String },
}
```

- [ ] **Step 4: Route service load**

In `crates/kat-rs-daemon/src/dataset_service.rs`, import:

```rust
use kat_rs_datasource::materialize_sqlite_dataset;
```

Add load variant:

```rust
enum DatasetLoad {
    Hitrace { path: PathBuf },
    LangfuseLegacy { observations_path: PathBuf, traces_path: PathBuf },
    Sqlite { path: PathBuf },
}
```

Map request:

```rust
DatasetSourceInput::Sqlite { file } => {
    let input = resolve_input(InputRole::File, file)?;
    Ok(DatasetLoad::Sqlite { path: input.path })
}
```

Run materialization:

```rust
DatasetLoad::Sqlite { path } => materialize_sqlite_dataset(path, dataset_path).await,
```

- [ ] **Step 5: Verify OpenAPI path list stays clean**

Run `openapi_endpoint_returns_current_api_paths`. When it reports a schema assertion mismatch caused by the `DatasetSourceInput` enum shape, update the schema assertion while keeping the path assertions unchanged. Do not add a `/v1/packs` or `/v1/sqlite` path in this task.

- [ ] **Step 6: Verify daemon tests**

Run:

```powershell
cargo test -p kat-rs-daemon dataset_create_materializes_sqlite_fixture_and_can_query_without_source
cargo test -p kat-rs-daemon openapi_endpoint_returns_current_api_paths
```

Expected: both PASS.

---

### Task 3: Minimal Python Worker and PackRunner

**Files:**
- Create: `python/kat_worker/kat_worker.py`
- Create: `crates/kat-rs-daemon/src/pack_runtime.rs`
- Modify: `crates/kat-rs-daemon/src/lib.rs`
- Test: `crates/kat-rs-daemon/tests/pack_runtime_contract.rs`

**Interfaces:**
- Produces: `PackRunner::new(dataset: TraceDatasource, config: PackRunnerConfig) -> PackRunner`
- Produces: `PackRunner::run(&self, request: PackRunRequest) -> Result<PackRunSummary, ApiError>`
- Produces: `PackRunRequest { pack_root: PathBuf, workflow_name: String, inputs: serde_json::Map<String, Value>, run_dir: PathBuf }`
- Produces: `PackRunSummary { status: PackRunStatus, artifacts: Vec<PackArtifactSummary>, logs: Vec<PackLogEntry>, traceback: Option<String> }`
- Produces: `PackArtifactSummary { name: String, query_id: String, row_count: usize, preview: serde_json::Value, path: PathBuf }`

- [ ] **Step 1: Write failing runner contract test**

Create `crates/kat-rs-daemon/tests/pack_runtime_contract.rs` with a simple pack fixture:

```rust
#[tokio::test]
async fn pack_runner_executes_workflow_and_materializes_returned_artifact() {
    let fixture = RuntimeFixture::new().await;
    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&fixture.dataset_path)
        .await
        .expect("dataset opens");
    let runner = kat_rs_daemon::pack_runtime::PackRunner::new(
        datasource,
        kat_rs_daemon::pack_runtime::PackRunnerConfig {
            python_executable: python_executable(),
            worker_script: workspace_root().join("python").join("kat_worker").join("kat_worker.py"),
            sdk_path: workspace_root().join("python").join("kat_sdk"),
            max_preview_rows: 50,
            max_bounded_rows: 1000,
        },
    );

    let summary = runner
        .run(kat_rs_daemon::pack_runtime::PackRunRequest {
            pack_root: fixture.pack_root.clone(),
            workflow_name: "workflows.extract".to_string(),
            inputs: serde_json::json!({ "min_id": 2 }).as_object().unwrap().clone(),
            run_dir: fixture.dir.path().join("run"),
        })
        .await
        .expect("pack run succeeds");

    assert_eq!(summary.status, kat_rs_daemon::pack_runtime::PackRunStatus::Succeeded);
    let artifact = summary
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "selected_threads")
        .expect("artifact exists");
    assert_eq!(artifact.row_count, 1);
    assert_eq!(artifact.preview, json!([{ "id": 2, "name": "worker" }]));
}
```

The fixture pack file `workflows/extract.py` should contain:

```python
from kat import option, workflow


@workflow(title="Select threads")
@option("--min-id", help="Minimum id", default=0)
def extract(min_id: int = 0):
    import kat

    rows = kat.query(
        "select id, name from thread where id >= :min_id order by id",
        min_id=min_id,
    )
    observed = rows.rows(max_rows=10)
    kat.log("selected rows", count=len(observed))
    return {"selected_threads": rows}
```

Add these test helpers in the same file:

```rust
struct RuntimeFixture {
    dir: TempDir,
    dataset_path: PathBuf,
    pack_root: PathBuf,
}

impl RuntimeFixture {
    async fn new() -> Self {
        let dir = tempdir().expect("runtime fixture tempdir");
        let sqlite_path = dir.path().join("trace.db");
        create_sqlite_runtime_fixture(&sqlite_path);

        let dataset_path = dir.path().join("dataset");
        kat_rs_datasource::materialize_sqlite_dataset(&sqlite_path, &dataset_path)
            .await
            .expect("sqlite dataset materializes");

        let pack_root = dir.path().join("pack");
        fs::create_dir_all(pack_root.join("workflows")).expect("pack workflows dir");
        fs::write(
            pack_root.join("workflows").join("extract.py"),
            r#"
from kat import option, workflow


@workflow(title="Select threads")
@option("--min-id", help="Minimum id", default=0)
def extract(min_id: int = 0):
    import kat

    rows = kat.query(
        "select id, name from thread where id >= :min_id order by id",
        min_id=min_id,
    )
    observed = rows.rows(max_rows=10)
    kat.log("selected rows", count=len(observed))
    return {"selected_threads": rows}
"#,
        )
        .expect("pack workflow is written");

        Self { dir, dataset_path, pack_root }
    }
}

fn create_sqlite_runtime_fixture(path: &Path) {
    let conn = rusqlite::Connection::open(path).expect("sqlite opens");
    conn.execute_batch(
        "
        create table thread(id int, name text);
        insert into thread values (1, 'main');
        insert into thread values (2, 'worker');
        ",
    )
    .expect("runtime fixture sqlite is created");
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("daemon crate has parent")
        .parent()
        .expect("workspace root exists")
        .to_path_buf()
}

fn python_executable() -> PathBuf {
    std::env::var_os("PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("python"))
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p kat-rs-daemon pack_runner_executes_workflow_and_materializes_returned_artifact
```

Expected: compile fails because `pack_runtime` and `kat_worker.py` do not exist.

- [ ] **Step 3: Implement worker protocol**

Create `python/kat_worker/kat_worker.py` with these top-level commands:

```python
def main() -> int:
    request = json.loads(sys.stdin.readline())
    if request.get("kind") != "run":
        write_message({"kind": "failed", "traceback": "first message must be run"})
        return 1
    return run_workflow(request)


if __name__ == "__main__":
    raise SystemExit(main())
```

Worker behavior:

- Insert `sdk_path` and `pack_root` into `sys.path`.
- Discover workflow files under `pack_root/**/*.py`.
- Import files using `importlib.util.spec_from_file_location`.
- Use `kat.get_workflow_spec(fn)` to find decorated functions.
- Derive workflow name from file path without `.py`, with path separators replaced by `.`.
- Bind runtime channel implementing `query`, `rows`, `preview`, and `log`.
- Call selected function with JSON inputs as keyword arguments.
- Validate return with `kat.validate_workflow_return`.
- Emit `{"kind":"complete","artifacts":{name: query_id}}`.

- [ ] **Step 4: Implement Rust request/response loop**

In `crates/kat-rs-daemon/src/pack_runtime.rs`, define:

```rust
#[derive(Clone, Debug)]
pub struct PackRunnerConfig {
    pub python_executable: PathBuf,
    pub worker_script: PathBuf,
    pub sdk_path: PathBuf,
    pub max_preview_rows: usize,
    pub max_bounded_rows: usize,
}

#[derive(Clone, Debug)]
pub struct PackRunRequest {
    pub pack_root: PathBuf,
    pub workflow_name: String,
    pub inputs: serde_json::Map<String, Value>,
    pub run_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackRunStatus {
    Succeeded,
    Failed,
}
```

Start Python with:

```rust
Command::new(&config.python_executable)
    .arg("-I")
    .arg(&config.worker_script)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
```

Send initial run message:

```json
{
  "kind": "run",
  "packRoot": "D:/work/kat_rs/0708/kat-rs/packs/critical-path",
  "workflowName": "workflows.extract",
  "inputs": {},
  "sdkPath": "D:/work/kat_rs/0708/kat-rs/python/kat_sdk",
  "runDir": "D:/tmp/kat-rs-runs/run-1"
}
```

- [ ] **Step 5: Implement query registry**

Store query records:

```rust
struct QueryRecord {
    sql: String,
    params: serde_json::Map<String, Value>,
}
```

For `query`, allocate `q1`, `q2`, and return it without executing SQL.

For `rows`, render params, wrap with limit:

```sql
select * from (<rendered sql>) as kat_query limit <max_rows + 1>
```

If returned rows exceed `max_rows`, return an error to worker.

For returned artifacts, execute the rendered SQL without an injected limit, save rows as:

```text
<run_dir>/artifacts/<artifact_name>.json
<run_dir>/artifacts/<artifact_name>.meta.json
```

The metadata JSON should contain:

```json
{
  "name": "path_nodes",
  "queryId": "q7",
  "rowCount": 12
}
```

- [ ] **Step 6: Implement SQL named parameter rendering**

Add tests in `pack_runtime_contract.rs`:

```rust
#[test]
fn sql_params_render_scalars_and_skip_string_literals() {
    let sql = "select ':not_param' as literal, :id as id, :name as name";
    let rendered = kat_rs_daemon::pack_runtime::render_sql_params(
        sql,
        &serde_json::json!({ "id": 7, "name": "O'Hara" }).as_object().unwrap().clone(),
    )
    .expect("params render");

    assert_eq!(rendered, "select ':not_param' as literal, 7 as id, 'O''Hara' as name");
}

#[test]
fn sql_params_reject_objects_and_arrays() {
    let error = kat_rs_daemon::pack_runtime::render_sql_params(
        "select :value",
        &serde_json::json!({ "value": [1, 2] }).as_object().unwrap().clone(),
    )
    .expect_err("array param is rejected");

    assert!(format!("{error:#}").contains("only scalar SQL params are supported"));
}
```

- [ ] **Step 7: Export module**

In `crates/kat-rs-daemon/src/lib.rs`:

```rust
pub mod pack_runtime;
```

- [ ] **Step 8: Verify runner contract**

Run:

```powershell
cargo test -p kat-rs-daemon pack_runner_executes_workflow_and_materializes_returned_artifact
cargo test -p kat-rs-daemon sql_params_
```

Expected: all runner contract tests pass.

---

### Task 4: critical-path Pack DataFusion Compatibility

**Files:**
- Modify: `packs/critical-path/lib/facts.py`
- Test: `tests/critical_path/test_engine.py`
- Test: `crates/kat-rs-daemon/tests/pack_runtime_contract.rs`

**Interfaces:**
- Consumes: existing `query_window_facts(kat, root_itid, root_tid, root_pid, root_thread_name_pattern, start_ts, end_ts, max_fact_rows)`
- Produces: DataFusion-compatible `WAKEUP_SQL` without SQLite `rowid`

- [ ] **Step 1: Add focused Python SQL contract test**

Create or extend a Python test in `tests/critical_path/test_engine.py`:

```python
def test_wakeup_sql_does_not_depend_on_sqlite_rowid(self) -> None:
    from lib.facts import WAKEUP_SQL

    self.assertNotIn("rowid", WAKEUP_SQL.lower())
    self.assertIn("CAST(NULL AS BIGINT) AS id", WAKEUP_SQL)
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
python -m unittest tests.critical_path.test_engine.CriticalPathEngineTests.test_wakeup_sql_does_not_depend_on_sqlite_rowid -v
```

Expected: FAIL because `WAKEUP_SQL` currently selects `rowid AS id`.

- [ ] **Step 3: Update WAKEUP_SQL**

Change `packs/critical-path/lib/facts.py`:

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
  AND ts >= :start_ts
  AND ts <= :end_ts
ORDER BY ts ASC
```

- [ ] **Step 4: Verify Python tests**

Run:

```powershell
python -m unittest discover -s tests\critical_path -v
```

Expected: all critical path Python tests pass.

---

### Task 5: Local test.db WeChat First-Frame Smoke

**Files:**
- Modify: `crates/kat-rs-daemon/tests/pack_runtime_contract.rs`

**Interfaces:**
- Consumes: `test/test.db` when present locally.
- Consumes: `packs/critical-path` workflow `workflows.extract`.
- Produces: ignored smoke test proving the MVP runs on the real fixture.

- [ ] **Step 1: Add ignored smoke test**

Add:

```rust
#[tokio::test]
#[ignore = "requires local test/test.db fixture"]
async fn critical_path_pack_extracts_wechat_first_frame_window_from_test_db() {
    let db_path = workspace_root().join("test").join("test.db");
    assert!(db_path.exists(), "missing local fixture: {}", db_path.display());

    let dir = tempdir().expect("tempdir");
    let dataset_path = dir.path().join("wechat-dataset");
    kat_rs_datasource::materialize_sqlite_dataset(&db_path, &dataset_path)
        .await
        .expect("test.db materializes");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let runner = kat_rs_daemon::pack_runtime::PackRunner::new(
        datasource,
        kat_rs_daemon::pack_runtime::PackRunnerConfig {
            python_executable: python_executable(),
            worker_script: workspace_root().join("python").join("kat_worker").join("kat_worker.py"),
            sdk_path: workspace_root().join("python").join("kat_sdk"),
            max_preview_rows: 50,
            max_bounded_rows: 100_000,
        },
    );

    let summary = runner
        .run(kat_rs_daemon::pack_runtime::PackRunRequest {
            pack_root: workspace_root().join("packs").join("critical-path"),
            workflow_name: "workflows.extract".to_string(),
            inputs: serde_json::json!({
                "root_itid": 405,
                "start_ts": 245615162000i64,
                "end_ts": 246306873000i64,
                "max_depth": 8,
                "min_segment_ms": 0.1,
                "max_fact_rows": 50000
            })
            .as_object()
            .unwrap()
            .clone(),
            run_dir: dir.path().join("wechat-run"),
        })
        .await
        .expect("critical path pack run succeeds");

    assert_eq!(summary.status, kat_rs_daemon::pack_runtime::PackRunStatus::Succeeded);
    assert_artifact_exists(&summary, "target_window");
    assert_artifact_exists(&summary, "path_nodes");
    assert_artifact_exists(&summary, "path_edges");
    assert_artifact_exists(&summary, "critical_path_evidence");
    assert_artifact_preview_contains(&summary, "target_window", "root_itid", json!(405));
    assert_artifact_preview_contains(&summary, "path_nodes", "itid", json!(405));
    assert_artifact_numeric_field_gt(&summary, "critical_path_evidence", "node_count", 0);
}
```

Add helper assertions in the same test file:

```rust
fn assert_artifact_exists(summary: &kat_rs_daemon::pack_runtime::PackRunSummary, name: &str) {
    assert!(
        summary.artifacts.iter().any(|artifact| artifact.name == name),
        "missing artifact {name}; artifacts: {:?}",
        summary.artifacts
    );
}

fn assert_artifact_preview_contains(
    summary: &kat_rs_daemon::pack_runtime::PackRunSummary,
    artifact_name: &str,
    key: &str,
    expected: serde_json::Value,
) {
    let artifact = summary
        .artifacts
        .iter()
        .find(|artifact| artifact.name == artifact_name)
        .expect("artifact exists");
    let rows = artifact.preview.as_array().expect("preview is an array");
    assert!(
        rows.iter().any(|row| row.get(key) == Some(&expected)),
        "artifact {artifact_name} preview does not contain {key}={expected}: {:?}",
        artifact.preview
    );
}

fn assert_artifact_numeric_field_gt(
    summary: &kat_rs_daemon::pack_runtime::PackRunSummary,
    artifact_name: &str,
    key: &str,
    min_value: u64,
) {
    let artifact = summary
        .artifacts
        .iter()
        .find(|artifact| artifact.name == artifact_name)
        .expect("artifact exists");
    let rows = artifact.preview.as_array().expect("preview is an array");
    let value = rows
        .first()
        .and_then(|row| row.get(key))
        .and_then(serde_json::Value::as_u64)
        .expect("numeric field exists");
    assert!(value > min_value, "{artifact_name}.{key}={value} <= {min_value}");
}
```

- [ ] **Step 2: Run ignored test to verify it fails before implementation is complete**

Run:

```powershell
cargo test -p kat-rs-daemon critical_path_pack_extracts_wechat_first_frame_window_from_test_db -- --ignored
```

Expected before Tasks 1-4 are complete: failure in materialization, runner, or pack SQL compatibility.

- [ ] **Step 3: Verify smoke passes after Tasks 1-4**

Run:

```powershell
cargo test -p kat-rs-daemon critical_path_pack_extracts_wechat_first_frame_window_from_test_db -- --ignored
```

Expected after implementation: PASS.

---

### Task 6: Full Verification and PR Hygiene

**Files:**
- Verify: all modified Rust/Python files.
- Verify: `docs/superpowers/specs/2026-07-08-kat-rs-python-pack-testdb-mvp-design.md`
- Verify: `docs/superpowers/plans/2026-07-08-kat-rs-python-pack-testdb-mvp.md`

**Interfaces:**
- Consumes: completed MVP implementation.
- Produces: verification evidence for PR.

- [ ] **Step 1: Run Python SDK tests**

Run:

```powershell
python -m unittest discover -s tests\kat_sdk -v
```

Expected: all tests pass.

- [ ] **Step 2: Run critical-path Python tests**

Run:

```powershell
python -m unittest discover -s tests\critical_path -v
```

Expected: all tests pass.

- [ ] **Step 3: Run datasource tests**

Run:

```powershell
cargo test -p kat-rs-datasource
```

Expected: all tests pass.

- [ ] **Step 4: Run daemon tests**

Run:

```powershell
cargo test -p kat-rs-daemon
```

Expected: all non-ignored tests pass.

- [ ] **Step 5: Run local test.db smoke**

Run:

```powershell
cargo test -p kat-rs-daemon critical_path_pack_extracts_wechat_first_frame_window_from_test_db -- --ignored
```

Expected: PASS on the local workspace where `test/test.db` exists.

- [ ] **Step 6: Confirm PR file scope**

Run:

```powershell
git diff --name-only origin/main...HEAD
```

Expected: no changes to `docs/critical-path.strategy.md` or `docs/superpowers/specs/2026-07-05-kat-rs-integrated-architecture-design.md`; `test/test.db` remains untracked unless explicitly approved.

- [ ] **Step 7: Commit**

Commit message:

```text
feat: run python critical path pack on sqlite dataset
```

Expected staged categories:

- datasource SQLite source and tests
- daemon dataset `SQLITE` source and tests
- minimal Python worker/PackRunner and tests
- critical-path DataFusion SQL compatibility fix
- MVP design and implementation plan

## Self-Review

- Spec coverage: Tasks 1-2 cover SQLite to Parquet catalog and daemon dataset input; Task 3 covers Python-first runner contract; Task 4 keeps critical-path generic and DataFusion-compatible; Task 5 covers local `test.db` WeChat first-frame validation; Task 6 covers verification and PR hygiene.
- Placeholder scan: no task relies on unspecified future behavior; ignored smoke test is explicit because `test/test.db` is a local fixture.
- Type consistency: function and struct names are consistent across tasks: `materialize_sqlite_dataset`, `PackRunner`, `PackRunnerConfig`, `PackRunRequest`, `PackRunSummary`, `PackRunStatus`.
