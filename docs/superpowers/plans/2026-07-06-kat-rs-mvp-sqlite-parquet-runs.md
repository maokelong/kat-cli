# kat-rs MVP SQLite Parquet Runs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the MVP path where `test/test.db` is materialized into the existing Parquet catalog dataset format, then executed through `POST /v1/runs` with the OpenHarmony critical task extraction pack.

**Architecture:** `kat-rs-datasource` owns SQLite reading and Parquet catalog materialization. `kat-rs-daemon` exposes the existing dataset REST surface plus new run REST APIs, and run execution only consumes catalog datasets through `TraceDatasource::from_dataset(...)`. Pack runtime stays in daemon and implements only the operators and control primitives required by `resources/packs/openharmony-critical-task-extraction`.

**Tech Stack:** Rust 2024, Axum, DataFusion, Arrow, Parquet, rusqlite, serde/serde_yaml, regex, sha2, uuid.

---

## File Structure

Create or modify these files:

- Modify: `Cargo.toml`
  - Add workspace dependencies used by the MVP: `rusqlite`, `regex`, `serde_yaml`, `sha2`, `hex`.
- Modify: `crates/kat-rs-datasource/Cargo.toml`
  - Add `rusqlite`.
- Modify: `crates/kat-rs-datasource/src/lib.rs`
  - Export `materialize_sqlite_dataset`.
- Modify: `crates/kat-rs-datasource/src/materializer.rs`
  - Dispatch to SQLite materialization.
- Create: `crates/kat-rs-datasource/src/formats/sqlite.rs`
  - SQLite-to-Arrow table extraction for the five MVP OpenHarmony tables.
- Modify: `crates/kat-rs-datasource/src/formats/mod.rs`
  - Register the SQLite format module.
- Modify: `crates/kat-rs-datasource/src/query.rs`
  - Expose `query_batches`, `query_json`, and `register_record_batches` so daemon run operators can execute DataFusion SQL and register run-local working tables.
- Test: `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`
  - Contract tests for SQLite materialization and DataFusion queryability.
- Modify: `crates/kat-rs-daemon/Cargo.toml`
  - Add `regex`, `serde_yaml`, `sha2`, `hex`, `datafusion`, `arrow-array`, `arrow-schema`.
- Modify: `crates/kat-rs-daemon/src/api.rs`
  - Add SQLite dataset input variant and run DTOs.
- Modify: `crates/kat-rs-daemon/src/dataset_service.rs`
  - Route `source: "SQLITE"` to datasource materialization.
- Modify: `crates/kat-rs-daemon/src/state.rs`
  - Add `RunService` to app state.
- Modify: `crates/kat-rs-daemon/src/lib.rs`
  - Export the run module.
- Modify: `crates/kat-rs-daemon/src/routes.rs`
  - Merge run routes.
- Create: `crates/kat-rs-daemon/src/routes/runs.rs`
  - Axum handlers for `/v1/runs`.
- Create: `crates/kat-rs-daemon/src/runs/mod.rs`
  - Public module boundary for run service types.
- Create: `crates/kat-rs-daemon/src/runs/service.rs`
  - Synchronous MVP run orchestration.
- Create: `crates/kat-rs-daemon/src/runs/store.rs`
  - In-memory run store.
- Create: `crates/kat-rs-daemon/src/runs/model.rs`
  - Run facts, evidence, brief, diagnostics, step records.
- Create: `crates/kat-rs-daemon/src/runs/context.rs`
  - Scalar/interval context store and publication records.
- Create: `crates/kat-rs-daemon/src/runs/resources.rs`
  - YAML loading for manifest, pack, flow, resource, summaries, brief.
- Create: `crates/kat-rs-daemon/src/runs/render.rs`
  - `{{ctx.slot}}` and `{{ctx.interval.start/end}}` rendering.
- Create: `crates/kat-rs-daemon/src/runs/operators.rs`
  - `grep`, `query`, `branch`, `loop`, and `summaries` execution.
- Modify: `crates/kat-rs-daemon/src/openapi.rs`
  - Add run paths and schemas.
- Test: `crates/kat-rs-daemon/tests/api_contract.rs`
  - Extend existing API contract tests.
- Test: `crates/kat-rs-daemon/tests/runs_contract.rs`
  - End-to-end dataset creation and run execution against `test/test.db`.
- Modify: `resources/openharmony/query/candidate_path_edges.yaml`
  - Add `rowid` to `requires.tables.instant.columns`.

---

### Task 1: Add Workspace Dependencies

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/kat-rs-datasource/Cargo.toml`
- Modify: `crates/kat-rs-daemon/Cargo.toml`

- [ ] **Step 1: Add workspace dependencies**

Edit the root `Cargo.toml` `[workspace.dependencies]` block and add:

```toml
hex = "0.4"
regex = "1.11"
rusqlite = { version = "0.37", features = ["bundled"] }
serde_yaml = "0.9"
sha2 = "0.10"
```

- [ ] **Step 2: Add datasource dependency**

Edit `crates/kat-rs-datasource/Cargo.toml` and add:

```toml
rusqlite.workspace = true
```

- [ ] **Step 3: Add daemon dependencies**

Edit `crates/kat-rs-daemon/Cargo.toml` and add:

```toml
arrow-array.workspace = true
arrow-schema.workspace = true
datafusion.workspace = true
hex.workspace = true
regex.workspace = true
serde_yaml.workspace = true
sha2.workspace = true
```

- [ ] **Step 4: Verify dependency resolution**

Run:

```powershell
cargo check -p kat-rs-datasource
cargo check -p kat-rs-daemon
```

Expected: both commands complete successfully. The first run may update `Cargo.lock`.

- [ ] **Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/kat-rs-datasource/Cargo.toml crates/kat-rs-daemon/Cargo.toml
git commit -m "chore: add mvp runtime dependencies"
```

---

### Task 2: Implement SQLite Dataset Materialization in Datasource

**Files:**
- Create: `crates/kat-rs-datasource/src/formats/sqlite.rs`
- Modify: `crates/kat-rs-datasource/src/formats/mod.rs`
- Modify: `crates/kat-rs-datasource/src/materializer.rs`
- Modify: `crates/kat-rs-datasource/src/lib.rs`
- Test: `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`

- [ ] **Step 1: Write failing datasource contract test**

Create `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`:

```rust
use std::path::Path;

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn sqlite_dataset_materializes_openharmony_tables_with_instant_rowid() {
    let dir = tempdir().expect("tempdir is created");
    let sqlite_path = dir.path().join("input.db");
    create_sqlite_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_sqlite_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    assert!(dataset_path.join("catalog.json").exists());

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json(
            "select \
               (select count(*) from process) as process_count, \
               (select count(*) from thread) as thread_count, \
               (select count(*) from callstack) as callstack_count, \
               (select count(*) from thread_state) as thread_state_count, \
               (select count(*) from instant) as instant_count, \
               (select rowid from instant where name = 'sched_wakeup') as wakeup_rowid",
        )
        .await
        .expect("dataset query succeeds");

    assert_eq!(
        rows,
        json!([{
            "process_count": 1,
            "thread_count": 1,
            "callstack_count": 1,
            "thread_state_count": 1,
            "instant_count": 1,
            "wakeup_rowid": 1
        }])
    );
}

fn create_sqlite_fixture(path: &Path) {
    let connection = Connection::open(path).expect("sqlite fixture opens");
    connection
        .execute_batch(
            r#"
            CREATE TABLE process (
                id INT, ipid INT, pid INT, name TEXT
            );
            CREATE TABLE thread (
                id INT, itid INT, tid INT, name TEXT, ipid INT, is_main_thread INT
            );
            CREATE TABLE callstack (
                id INT, ts INT, dur INT, callid INT, name TEXT, depth INT, parent_id INT
            );
            CREATE TABLE thread_state (
                id INT, ts INT, dur INT, itid INT, tid INT, state TEXT
            );
            CREATE TABLE instant (
                ts INT, name TEXT, ref INT, wakeup_from INT, ref_type TEXT
            );

            INSERT INTO process VALUES (89, 89, 15040, '.tencent.wechat');
            INSERT INTO thread VALUES (405, 405, 15040, '.tencent.wechat', 89, 1);
            INSERT INTO callstack VALUES (6387, 245720189000, 481901000, 405, 'HandleLaunchAbility', 0, 4294967295);
            INSERT INTO thread_state VALUES (1, 245720189000, 1000000, 405, 15040, 'Sleeping');
            INSERT INTO instant VALUES (245721000000, 'sched_wakeup', 405, 406, 'itid');
            "#,
        )
        .expect("sqlite fixture schema is created");
}
```

- [ ] **Step 2: Run the failing test**

Run:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- sqlite_dataset_materializes_openharmony_tables_with_instant_rowid --nocapture
```

Expected: FAIL because `kat_rs_datasource::materialize_sqlite_dataset` is not exported.

- [ ] **Step 3: Register SQLite format module**

Modify `crates/kat-rs-datasource/src/formats/mod.rs`:

```rust
pub(crate) mod hitrace;
pub(crate) mod langfuse;
pub(crate) mod sqlite;
```

Keep any existing module declarations and add only `sqlite`.

- [ ] **Step 4: Implement SQLite table extraction**

Create `crates/kat-rs-datasource/src/formats/sqlite.rs`:

```rust
use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use arrow_array::{
    ArrayRef, Int64Array, RecordBatch, StringArray,
    builder::{Int64Builder, StringBuilder},
};
use arrow_schema::{DataType, Field, Schema};
use rusqlite::{Connection, types::ValueRef};

pub(crate) struct SqliteTable {
    pub(crate) logical_name: &'static str,
    pub(crate) parquet_file_name: String,
    pub(crate) batch: RecordBatch,
}

#[derive(Clone, Copy)]
enum SqliteColumnType {
    Int64,
    Utf8,
}

struct SqliteColumn {
    output_name: &'static str,
    select_expr: &'static str,
    data_type: SqliteColumnType,
}

struct SqliteTableSpec {
    logical_name: &'static str,
    columns: &'static [SqliteColumn],
}

const PROCESS_COLUMNS: &[SqliteColumn] = &[
    SqliteColumn { output_name: "id", select_expr: "id", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "ipid", select_expr: "ipid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "pid", select_expr: "pid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "name", select_expr: "name", data_type: SqliteColumnType::Utf8 },
];

const THREAD_COLUMNS: &[SqliteColumn] = &[
    SqliteColumn { output_name: "id", select_expr: "id", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "itid", select_expr: "itid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "tid", select_expr: "tid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "name", select_expr: "name", data_type: SqliteColumnType::Utf8 },
    SqliteColumn { output_name: "ipid", select_expr: "ipid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "is_main_thread", select_expr: "is_main_thread", data_type: SqliteColumnType::Int64 },
];

const CALLSTACK_COLUMNS: &[SqliteColumn] = &[
    SqliteColumn { output_name: "id", select_expr: "id", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "ts", select_expr: "ts", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "dur", select_expr: "dur", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "callid", select_expr: "callid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "name", select_expr: "name", data_type: SqliteColumnType::Utf8 },
    SqliteColumn { output_name: "depth", select_expr: "depth", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "parent_id", select_expr: "parent_id", data_type: SqliteColumnType::Int64 },
];

const THREAD_STATE_COLUMNS: &[SqliteColumn] = &[
    SqliteColumn { output_name: "id", select_expr: "id", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "ts", select_expr: "ts", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "dur", select_expr: "dur", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "itid", select_expr: "itid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "tid", select_expr: "tid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "state", select_expr: "state", data_type: SqliteColumnType::Utf8 },
];

const INSTANT_COLUMNS: &[SqliteColumn] = &[
    SqliteColumn { output_name: "rowid", select_expr: "rowid", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "ts", select_expr: "ts", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "name", select_expr: "name", data_type: SqliteColumnType::Utf8 },
    SqliteColumn { output_name: "ref", select_expr: "ref", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "wakeup_from", select_expr: "wakeup_from", data_type: SqliteColumnType::Int64 },
    SqliteColumn { output_name: "ref_type", select_expr: "ref_type", data_type: SqliteColumnType::Utf8 },
];

const TABLES: &[SqliteTableSpec] = &[
    SqliteTableSpec { logical_name: "process", columns: PROCESS_COLUMNS },
    SqliteTableSpec { logical_name: "thread", columns: THREAD_COLUMNS },
    SqliteTableSpec { logical_name: "callstack", columns: CALLSTACK_COLUMNS },
    SqliteTableSpec { logical_name: "thread_state", columns: THREAD_STATE_COLUMNS },
    SqliteTableSpec { logical_name: "instant", columns: INSTANT_COLUMNS },
];

pub(crate) fn openharmony_tables(path: &Path) -> Result<Vec<SqliteTable>> {
    let connection = Connection::open(path)
        .with_context(|| format!("failed to open SQLite dataset source: {}", path.display()))?;
    TABLES
        .iter()
        .map(|spec| read_table(&connection, spec))
        .collect()
}

fn read_table(connection: &Connection, spec: &SqliteTableSpec) -> Result<SqliteTable> {
    let projection = spec
        .columns
        .iter()
        .map(|column| format!("{} AS {}", column.select_expr, column.output_name))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {projection} FROM {}", spec.logical_name);
    let mut statement = connection
        .prepare(&sql)
        .with_context(|| format!("failed to prepare SQLite table query for {}", spec.logical_name))?;
    let mut rows = statement
        .query([])
        .with_context(|| format!("failed to read SQLite table {}", spec.logical_name))?;

    let mut columns = spec
        .columns
        .iter()
        .map(|column| ColumnBuilder::new(column.data_type))
        .collect::<Vec<_>>();

    while let Some(row) = rows
        .next()
        .with_context(|| format!("failed to advance SQLite rows for {}", spec.logical_name))?
    {
        for (index, builder) in columns.iter_mut().enumerate() {
            builder.append(row.get_ref(index)?)?;
        }
    }

    let fields = spec
        .columns
        .iter()
        .map(|column| {
            Field::new(
                column.output_name,
                match column.data_type {
                    SqliteColumnType::Int64 => DataType::Int64,
                    SqliteColumnType::Utf8 => DataType::Utf8,
                },
                true,
            )
        })
        .collect::<Vec<_>>();
    let arrays = columns
        .into_iter()
        .map(ColumnBuilder::finish)
        .collect::<Vec<_>>();
    let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .with_context(|| format!("failed to build Arrow batch for {}", spec.logical_name))?;

    Ok(SqliteTable {
        logical_name: spec.logical_name,
        parquet_file_name: format!("sqlite.{}.parquet", spec.logical_name),
        batch,
    })
}

enum ColumnBuilder {
    Int64(Int64Builder),
    Utf8(StringBuilder),
}

impl ColumnBuilder {
    fn new(data_type: SqliteColumnType) -> Self {
        match data_type {
            SqliteColumnType::Int64 => Self::Int64(Int64Builder::new()),
            SqliteColumnType::Utf8 => Self::Utf8(StringBuilder::new()),
        }
    }

    fn append(&mut self, value: ValueRef<'_>) -> Result<()> {
        match self {
            Self::Int64(builder) => append_int64(builder, value),
            Self::Utf8(builder) => append_utf8(builder, value),
        }
    }

    fn finish(self) -> ArrayRef {
        match self {
            Self::Int64(mut builder) => Arc::new(Int64Array::from(builder.finish())),
            Self::Utf8(mut builder) => Arc::new(StringArray::from(builder.finish())),
        }
    }
}

fn append_int64(builder: &mut Int64Builder, value: ValueRef<'_>) -> Result<()> {
    match value {
        ValueRef::Null => builder.append_null(),
        ValueRef::Integer(value) => builder.append_value(value),
        ValueRef::Real(value) => builder.append_value(value as i64),
        other => bail!("expected SQLite integer value, got {other:?}"),
    }
    Ok(())
}

fn append_utf8(builder: &mut StringBuilder, value: ValueRef<'_>) -> Result<()> {
    match value {
        ValueRef::Null => builder.append_null(),
        ValueRef::Text(value) => builder.append_value(std::str::from_utf8(value)?),
        ValueRef::Integer(value) => builder.append_value(value.to_string()),
        ValueRef::Real(value) => builder.append_value(value.to_string()),
        ValueRef::Blob(_) => bail!("expected SQLite text value, got blob"),
    }
    Ok(())
}
```

- [ ] **Step 5: Add materializer function**

Modify `crates/kat-rs-datasource/src/materializer.rs` imports:

```rust
use crate::{
    arrow_table::ArrowTable,
    dataset::{DatasetTableWriter, DatasetWriter},
    formats::{hitrace, langfuse, sqlite},
    record::{TraceRecord, TraceRecordSink},
    sinks::arrow::ArrowSink,
};
```

Add this public function after `materialize_langfuse_legacy_dataset`:

```rust
pub async fn materialize_sqlite_dataset(
    path: impl AsRef<Path>,
    dataset_path: impl AsRef<Path>,
) -> Result<()> {
    let path = path.as_ref();
    let dataset_path = dataset_path.as_ref();

    let mut writer = DatasetWriter::create(dataset_path)?;
    for table in sqlite::openharmony_tables(path).with_context(|| {
        format!(
            "failed to read OpenHarmony SQLite dataset source: {}",
            path.display()
        )
    })? {
        let mut table_writer = writer.start_table(
            table.logical_name,
            &table.parquet_file_name,
            table.batch.schema(),
        )?;
        table_writer.write(&table.batch)?;
        writer.add_table(table_writer.finish()?);
    }

    writer.finish().await
}
```

- [ ] **Step 6: Export materializer**

Modify `crates/kat-rs-datasource/src/lib.rs`:

```rust
pub use materializer::{
    materialize_hitrace_dataset, materialize_langfuse_legacy_dataset, materialize_sqlite_dataset,
};
```

- [ ] **Step 7: Run datasource contract test**

Run:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- sqlite_dataset_materializes_openharmony_tables_with_instant_rowid --nocapture
```

Expected: PASS.

- [ ] **Step 8: Run datasource tests**

Run:

```powershell
cargo test -p kat-rs-datasource
```

Expected: PASS.

- [ ] **Step 9: Commit**

```powershell
git add crates/kat-rs-datasource/src/formats/mod.rs crates/kat-rs-datasource/src/formats/sqlite.rs crates/kat-rs-datasource/src/materializer.rs crates/kat-rs-datasource/src/lib.rs crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs
git commit -m "feat: materialize sqlite openharmony datasets"
```

---

### Task 3: Wire SQLite Source Through `POST /v1/datasets`

**Files:**
- Modify: `crates/kat-rs-daemon/src/api.rs`
- Modify: `crates/kat-rs-daemon/src/dataset_service.rs`
- Modify: `crates/kat-rs-daemon/src/openapi.rs`
- Test: `crates/kat-rs-daemon/tests/api_contract.rs`

- [ ] **Step 1: Add failing API test**

Add this test to `crates/kat-rs-daemon/tests/api_contract.rs`:

```rust
#[tokio::test]
async fn dataset_create_materializes_sqlite_fixture_and_can_query_catalog() {
    let dir = tempdir().expect("tempdir is created");
    let sqlite_path = dir.path().join("input.db");
    create_openharmony_sqlite_fixture(&sqlite_path);
    let datasets_root = dir.path().join("datasets");
    let dataset_name = "sqlite-openharmony";
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
                "file": sqlite_path.to_string_lossy(),
            }
        })),
    )
    .await;

    assert_eq!(create.status, StatusCode::CREATED, "{:?}", create.body);
    assert_eq!(create.body["data"]["dataset"]["name"], dataset_name);
    assert_eq!(
        create.body["data"]["dataset"]["path"],
        dataset_path.to_string_lossy().as_ref()
    );

    let query = request_json(
        app,
        "POST",
        "/v1/datasets/queries",
        Some(json!({
            "dataset": {
                "name": dataset_name,
                "directory": datasets_root.to_string_lossy(),
            },
            "sql": "select count(*) as process_count from process"
        })),
    )
    .await;

    assert_eq!(query.status, StatusCode::OK, "{:?}", query.body);
    assert_eq!(query.body["data"], json!([{ "process_count": 1 }]));
}

fn create_openharmony_sqlite_fixture(path: &Path) {
    let connection = rusqlite::Connection::open(path).expect("sqlite fixture opens");
    connection
        .execute_batch(
            r#"
            CREATE TABLE process (id INT, ipid INT, pid INT, name TEXT);
            CREATE TABLE thread (id INT, itid INT, tid INT, name TEXT, ipid INT, is_main_thread INT);
            CREATE TABLE callstack (id INT, ts INT, dur INT, callid INT, name TEXT, depth INT, parent_id INT);
            CREATE TABLE thread_state (id INT, ts INT, dur INT, itid INT, tid INT, state TEXT);
            CREATE TABLE instant (ts INT, name TEXT, ref INT, wakeup_from INT, ref_type TEXT);
            INSERT INTO process VALUES (89, 89, 15040, '.tencent.wechat');
            INSERT INTO thread VALUES (405, 405, 15040, '.tencent.wechat', 89, 1);
            INSERT INTO callstack VALUES (6387, 245720189000, 481901000, 405, 'HandleLaunchAbility', 0, 4294967295);
            INSERT INTO thread_state VALUES (1, 245720189000, 1000000, 405, 15040, 'Sleeping');
            INSERT INTO instant VALUES (245721000000, 'sched_wakeup', 405, 406, 'itid');
            "#,
        )
        .expect("sqlite fixture schema is created");
}
```

Add this import near the top of the test file:

```rust
use rusqlite;
```

- [ ] **Step 2: Run the failing API test**

Run:

```powershell
cargo test -p kat-rs-daemon --test api_contract -- dataset_create_materializes_sqlite_fixture_and_can_query_catalog --nocapture
```

Expected: FAIL because `SQLITE` is not a known dataset input source.

- [ ] **Step 3: Add SQLite dataset input DTO**

Modify `crates/kat-rs-daemon/src/api.rs`:

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

- [ ] **Step 4: Route SQLite input through dataset service**

Modify imports in `crates/kat-rs-daemon/src/dataset_service.rs`:

```rust
use kat_rs_datasource::{
    DatasetLocator, DatasetStore, TraceDatasource, inspect_dataset_tables,
    materialize_hitrace_dataset, materialize_langfuse_legacy_dataset, materialize_sqlite_dataset,
};
```

Extend `DatasetLoad`:

```rust
enum DatasetLoad {
    Hitrace {
        path: PathBuf,
    },
    LangfuseLegacy {
        observations_path: PathBuf,
        traces_path: PathBuf,
    },
    Sqlite {
        path: PathBuf,
    },
}
```

Extend `dataset_load`:

```rust
DatasetSourceInput::Sqlite { file } => {
    let input = resolve_input(InputRole::File, file)?;
    Ok(DatasetLoad::Sqlite { path: input.path })
}
```

Extend `materialize_dataset`:

```rust
DatasetLoad::Sqlite { path } => materialize_sqlite_dataset(path, dataset_path).await,
```

- [ ] **Step 5: Update OpenAPI schema coverage test**

Run:

```powershell
cargo test -p kat-rs-daemon --test api_contract -- openapi_endpoint_returns_current_api_paths --nocapture
```

Expected: PASS. `DatasetSourceInput` already appears in OpenAPI components; no path assertion changes are needed for this task.

- [ ] **Step 6: Run daemon API tests**

Run:

```powershell
cargo test -p kat-rs-daemon --test api_contract -- dataset_create_materializes_sqlite_fixture_and_can_query_catalog --nocapture
cargo test -p kat-rs-daemon --test api_contract
```

Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add crates/kat-rs-daemon/src/api.rs crates/kat-rs-daemon/src/dataset_service.rs crates/kat-rs-daemon/tests/api_contract.rs
git commit -m "feat: expose sqlite dataset materialization"
```

---

### Task 4: Expose DataFusion Batch APIs for Run Operators

**Files:**
- Modify: `crates/kat-rs-datasource/src/query.rs`
- Test: `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`

- [ ] **Step 1: Add failing test for run-local table registration**

Append this test to `crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs`:

```rust
#[tokio::test]
async fn trace_datasource_registers_run_local_record_batches() {
    let dir = tempdir().expect("tempdir is created");
    let sqlite_path = dir.path().join("input.db");
    create_sqlite_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");
    kat_rs_datasource::materialize_sqlite_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let batches = datasource
        .query_batches("select id, name from process")
        .await
        .expect("source query returns batches");
    datasource
        .register_record_batches("process_copy", batches)
        .expect("run-local table registers");

    let rows = datasource
        .query_json("select name from process_copy")
        .await
        .expect("registered table is queryable");

    assert_eq!(rows, json!([{ "name": ".tencent.wechat" }]));
}
```

- [ ] **Step 2: Run failing test**

Run:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- trace_datasource_registers_run_local_record_batches --nocapture
```

Expected: FAIL because `query_batches` and `register_record_batches` do not exist.

- [ ] **Step 3: Implement batch APIs**

Modify `crates/kat-rs-datasource/src/query.rs` imports:

```rust
use datafusion::{
    datasource::{MemTable, file_format::file_compression_type::FileCompressionType},
    prelude::{JsonReadOptions, SessionContext},
};
use arrow_array::RecordBatch;
```

Add these methods to `impl TraceDatasource`:

```rust
pub async fn query_batches(&self, sql: &str) -> Result<Vec<RecordBatch>> {
    debug!("running datasource sql: {sql}");
    let dataframe = self.ctx.sql(sql).await?;
    dataframe.collect().await.map_err(Into::into)
}

pub fn register_record_batches(&self, table_name: &str, batches: Vec<RecordBatch>) -> Result<()> {
    let schema = batches
        .first()
        .with_context(|| format!("run-local table {table_name} produced no record batches"))?
        .schema();
    let mem_table = MemTable::try_new(schema, vec![batches])?;
    self.ctx.register_table(table_name, Arc::new(mem_table))?;
    Ok(())
}
```

Update `query_json` to call `query_batches`:

```rust
pub async fn query_json(&self, sql: &str) -> Result<Value> {
    let batches = self.query_batches(sql).await?;
    batches_to_json(&batches)
}
```

- [ ] **Step 4: Run datasource tests**

Run:

```powershell
cargo test -p kat-rs-datasource --test sqlite_dataset_contract -- trace_datasource_registers_run_local_record_batches --nocapture
cargo test -p kat-rs-datasource
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/kat-rs-datasource/src/query.rs crates/kat-rs-datasource/tests/sqlite_dataset_contract.rs
git commit -m "feat: expose run local datasource tables"
```

---

### Task 5: Add Run API DTOs, In-Memory Store, and Routes

**Files:**
- Modify: `crates/kat-rs-daemon/src/api.rs`
- Modify: `crates/kat-rs-daemon/src/lib.rs`
- Modify: `crates/kat-rs-daemon/src/state.rs`
- Modify: `crates/kat-rs-daemon/src/routes.rs`
- Create: `crates/kat-rs-daemon/src/routes/runs.rs`
- Create: `crates/kat-rs-daemon/src/runs/mod.rs`
- Create: `crates/kat-rs-daemon/src/runs/model.rs`
- Create: `crates/kat-rs-daemon/src/runs/store.rs`
- Create: `crates/kat-rs-daemon/src/runs/service.rs`
- Modify: `crates/kat-rs-daemon/src/openapi.rs`
- Test: `crates/kat-rs-daemon/tests/runs_contract.rs`

- [ ] **Step 1: Write failing route contract test**

Create `crates/kat-rs-daemon/tests/runs_contract.rs`:

```rust
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn run_endpoint_returns_not_found_for_unknown_run() {
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());
    let response = request_json(app, "GET", "/v1/runs/run_missing", None).await;

    assert_eq!(response.status, StatusCode::NOT_FOUND, "{:?}", response.body);
    assert_eq!(response.body["error"]["code"], "VALIDATION_FAILED");
}

struct JsonResponse {
    status: StatusCode,
    body: serde_json::Value,
}

async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> JsonResponse {
    let body = body
        .map(|body| Body::from(serde_json::to_vec(&body).expect("json body serializes")))
        .unwrap_or_else(Body::empty);
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builds");
    let response = app.oneshot(request).await.expect("response is returned");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let body = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).expect("json body")
    };

    JsonResponse { status, body }
}
```

- [ ] **Step 2: Run failing route test**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- run_endpoint_returns_not_found_for_unknown_run --nocapture
```

Expected: FAIL because `/v1/runs/{runId}` is not routed.

- [ ] **Step 3: Add run DTOs**

Append to `crates/kat-rs-daemon/src/api.rs`:

```rust
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRunRequest {
    pub pack_ref: String,
    pub dataset: DatasetLocation,
    pub inputs: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryDto {
    pub run_id: String,
    pub status: String,
    pub pack_ref: String,
    pub dataset: DatasetDto,
    pub step_count: usize,
    pub evidence_count: usize,
    pub brief_section_count: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunDetailDto {
    pub summary: RunSummaryDto,
    pub steps: Vec<RunStepDto>,
    pub diagnostics: Vec<Value>,
    pub snapshot_digest: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunStepDto {
    pub id: String,
    pub uses: String,
    pub status: String,
    pub output: Option<String>,
    pub row_count: Option<usize>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidenceResponse {
    pub run_id: String,
    pub evidence: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunBriefResponse {
    pub run_id: String,
    pub sections: Vec<Value>,
}
```

- [ ] **Step 4: Add run model and store**

Create `crates/kat-rs-daemon/src/runs/model.rs`:

```rust
use serde_json::Value;

use crate::api::{DatasetDto, RunBriefResponse, RunDetailDto, RunEvidenceResponse, RunStepDto, RunSummaryDto};

#[derive(Clone, Debug)]
pub struct RunRecord {
    pub run_id: String,
    pub status: String,
    pub pack_ref: String,
    pub dataset: DatasetDto,
    pub snapshot_digest: String,
    pub steps: Vec<RunStepRecord>,
    pub diagnostics: Vec<Value>,
    pub evidence: Vec<Value>,
    pub brief_sections: Vec<Value>,
}

#[derive(Clone, Debug)]
pub struct RunStepRecord {
    pub id: String,
    pub uses: String,
    pub status: String,
    pub output: Option<String>,
    pub row_count: Option<usize>,
}

impl RunRecord {
    pub fn summary(&self) -> RunSummaryDto {
        RunSummaryDto {
            run_id: self.run_id.clone(),
            status: self.status.clone(),
            pack_ref: self.pack_ref.clone(),
            dataset: self.dataset.clone(),
            step_count: self.steps.len(),
            evidence_count: self.evidence.len(),
            brief_section_count: self.brief_sections.len(),
        }
    }

    pub fn detail(&self) -> RunDetailDto {
        RunDetailDto {
            summary: self.summary(),
            steps: self.steps.iter().map(RunStepRecord::dto).collect(),
            diagnostics: self.diagnostics.clone(),
            snapshot_digest: self.snapshot_digest.clone(),
        }
    }

    pub fn evidence_response(&self) -> RunEvidenceResponse {
        RunEvidenceResponse {
            run_id: self.run_id.clone(),
            evidence: self.evidence.clone(),
        }
    }

    pub fn brief_response(&self) -> RunBriefResponse {
        RunBriefResponse {
            run_id: self.run_id.clone(),
            sections: self.brief_sections.clone(),
        }
    }
}

impl RunStepRecord {
    pub fn dto(&self) -> RunStepDto {
        RunStepDto {
            id: self.id.clone(),
            uses: self.uses.clone(),
            status: self.status.clone(),
            output: self.output.clone(),
            row_count: self.row_count,
        }
    }
}
```

Create `crates/kat-rs-daemon/src/runs/store.rs`:

```rust
use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

use super::model::RunRecord;

#[derive(Default)]
pub struct RunStore {
    runs: RwLock<HashMap<String, Arc<RunRecord>>>,
}

impl RunStore {
    pub async fn insert(&self, run: RunRecord) -> Arc<RunRecord> {
        let run = Arc::new(run);
        self.runs
            .write()
            .await
            .insert(run.run_id.clone(), Arc::clone(&run));
        run
    }

    pub async fn get(&self, run_id: &str) -> Option<Arc<RunRecord>> {
        self.runs.read().await.get(run_id).cloned()
    }
}
```

- [ ] **Step 5: Add skeletal run service**

Create `crates/kat-rs-daemon/src/runs/service.rs`:

```rust
use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::{
    api::{CreateRunRequest, DatasetDto, DatasetLocation},
    error::ApiError,
};

use super::{
    model::{RunRecord, RunStepRecord},
    store::RunStore,
};

#[derive(Default)]
pub struct RunService {
    store: Arc<RunStore>,
}

impl RunService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_placeholder(
        &self,
        request: CreateRunRequest,
        dataset: DatasetDto,
    ) -> Result<Arc<RunRecord>, ApiError> {
        let run_id = format!("run_{}", Uuid::now_v7().simple());
        let record = RunRecord {
            run_id,
            status: "FAILED".to_owned(),
            pack_ref: request.pack_ref,
            dataset,
            snapshot_digest: "sha256:not-executed".to_owned(),
            steps: vec![RunStepRecord {
                id: "pack_runtime_not_implemented".to_owned(),
                uses: "runtime".to_owned(),
                status: "FAILED".to_owned(),
                output: None,
                row_count: None,
            }],
            diagnostics: vec![json!({
                "code": "PACK_RUNTIME_NOT_IMPLEMENTED",
                "message": "pack runtime will be implemented in the next task"
            })],
            evidence: Vec::new(),
            brief_sections: Vec::new(),
        };

        Ok(self.store.insert(record).await)
    }

    pub async fn get(&self, run_id: &str) -> Result<Arc<RunRecord>, ApiError> {
        self.store
            .get(run_id)
            .await
            .ok_or_else(|| ApiError::validation(format!("run not found: {run_id}")))
    }
}
```

Create `crates/kat-rs-daemon/src/runs/mod.rs`:

```rust
pub mod context;
pub mod model;
pub mod operators;
pub mod render;
pub mod resources;
pub mod service;
pub mod store;

pub use service::RunService;
```

For this task, create empty files so the module compiles:

```rust
// crates/kat-rs-daemon/src/runs/context.rs
```

```rust
// crates/kat-rs-daemon/src/runs/operators.rs
```

```rust
// crates/kat-rs-daemon/src/runs/render.rs
```

```rust
// crates/kat-rs-daemon/src/runs/resources.rs
```

- [ ] **Step 6: Add service to app state**

Modify `crates/kat-rs-daemon/src/state.rs`:

```rust
use crate::{dataset_service::DatasetService, runs::RunService, service::DatasourceService};
```

Add field:

```rust
pub run_service: Arc<RunService>,
```

Initialize it in `AppState::new`:

```rust
run_service: Arc::new(RunService::new()),
```

Modify `crates/kat-rs-daemon/src/lib.rs`:

```rust
pub mod runs;
```

- [ ] **Step 7: Add run routes**

Create `crates/kat-rs-daemon/src/routes/runs.rs`:

```rust
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    routing::{get, post},
};

use crate::{
    api::{CreateRunRequest, DataEnvelope, RunBriefResponse, RunDetailDto, RunEvidenceResponse, RunSummaryDto},
    error::{ApiError, ErrorEnvelope},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/runs", post(create_run))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/evidence", get(get_run_evidence))
        .route("/v1/runs/{run_id}/brief", get(get_run_brief))
}

#[utoipa::path(
    post,
    path = "/v1/runs",
    request_body = CreateRunRequest,
    responses(
        (status = 200, description = "Run submitted and executed", body = DataEnvelope<RunSummaryDto>),
        (status = 400, description = "Request body is malformed", body = ErrorEnvelope),
        (status = 422, description = "Run failed validation", body = ErrorEnvelope)
    )
)]
pub(crate) async fn create_run(
    State(state): State<AppState>,
    request: Result<Json<CreateRunRequest>, JsonRejection>,
) -> Result<Json<DataEnvelope<RunSummaryDto>>, ApiError> {
    let Json(request) =
        request.map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;
    let dataset = state.dataset_service.resolve_existing(request.dataset.clone())?;
    let run = state.run_service.create_placeholder(request, dataset).await?;
    Ok(Json(DataEnvelope { data: run.summary() }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}",
    params(("runId" = String, Path, description = "Run id")),
    responses((status = 200, description = "Run detail", body = DataEnvelope<RunDetailDto>))
)]
pub(crate) async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunDetailDto>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;
    Ok(Json(DataEnvelope { data: run.detail() }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}/evidence",
    params(("runId" = String, Path, description = "Run id")),
    responses((status = 200, description = "Run evidence", body = DataEnvelope<RunEvidenceResponse>))
)]
pub(crate) async fn get_run_evidence(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunEvidenceResponse>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;
    Ok(Json(DataEnvelope { data: run.evidence_response() }))
}

#[utoipa::path(
    get,
    path = "/v1/runs/{runId}/brief",
    params(("runId" = String, Path, description = "Run id")),
    responses((status = 200, description = "Run brief", body = DataEnvelope<RunBriefResponse>))
)]
pub(crate) async fn get_run_brief(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<DataEnvelope<RunBriefResponse>>, ApiError> {
    let run = state.run_service.get(&run_id).await?;
    Ok(Json(DataEnvelope { data: run.brief_response() }))
}
```

Add `resolve_existing` to `DatasetService` in `crates/kat-rs-daemon/src/dataset_service.rs`:

```rust
pub fn resolve_existing(&self, dataset: DatasetLocation) -> Result<DatasetDto, ApiError> {
    let resolved = resolve_dataset(&dataset)?;
    ensure_dataset_exists(&resolved.path)?;
    Ok(resolved.dataset)
}
```

Modify `crates/kat-rs-daemon/src/routes.rs`:

```rust
pub(crate) mod runs;
```

and merge routes:

```rust
.merge(runs::routes())
```

- [ ] **Step 8: Update OpenAPI**

Modify `crates/kat-rs-daemon/src/openapi.rs` imports and schema list to include:

```rust
CreateRunRequest,
RunBriefResponse,
RunDetailDto,
RunEvidenceResponse,
RunStepDto,
RunSummaryDto,
```

Add paths:

```rust
crate::routes::runs::create_run,
crate::routes::runs::get_run,
crate::routes::runs::get_run_evidence,
crate::routes::runs::get_run_brief,
```

- [ ] **Step 9: Run route tests**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- run_endpoint_returns_not_found_for_unknown_run --nocapture
cargo test -p kat-rs-daemon --test api_contract -- openapi_endpoint_returns_current_api_paths --nocapture
```

Expected: first test PASS. The OpenAPI test may fail until assertions are updated.

- [ ] **Step 10: Update OpenAPI assertions**

In `openapi_endpoint_returns_current_api_paths`, add:

```rust
assert!(value["paths"]["/v1/runs"]["post"].is_object());
assert!(value["paths"]["/v1/runs/{runId}"]["get"].is_object());
assert!(value["paths"]["/v1/runs/{runId}/evidence"]["get"].is_object());
assert!(value["paths"]["/v1/runs/{runId}/brief"]["get"].is_object());
```

Add these schema names to the schema loop:

```rust
"CreateRunRequest",
"RunBriefResponse",
"RunDetailDto",
"RunEvidenceResponse",
"RunStepDto",
"RunSummaryDto",
```

- [ ] **Step 11: Run daemon tests**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract
cargo test -p kat-rs-daemon --test api_contract
```

Expected: PASS.

- [ ] **Step 12: Commit**

```powershell
git add crates/kat-rs-daemon/src/api.rs crates/kat-rs-daemon/src/lib.rs crates/kat-rs-daemon/src/state.rs crates/kat-rs-daemon/src/routes.rs crates/kat-rs-daemon/src/routes/runs.rs crates/kat-rs-daemon/src/runs crates/kat-rs-daemon/src/openapi.rs crates/kat-rs-daemon/src/dataset_service.rs crates/kat-rs-daemon/tests/api_contract.rs crates/kat-rs-daemon/tests/runs_contract.rs
git commit -m "feat: add run api skeleton"
```

---

### Task 6: Implement Pack Resource Loading, Context, and SQL Rendering

**Files:**
- Modify: `crates/kat-rs-daemon/src/runs/context.rs`
- Modify: `crates/kat-rs-daemon/src/runs/render.rs`
- Modify: `crates/kat-rs-daemon/src/runs/resources.rs`
- Test: `crates/kat-rs-daemon/tests/runs_contract.rs`

- [ ] **Step 1: Add focused tests for context and rendering**

Append to `crates/kat-rs-daemon/tests/runs_contract.rs`:

```rust
#[test]
fn context_renderer_replaces_scalar_and_interval_slots() {
    let mut context = kat_rs_daemon::runs::context::ContextStore::new();
    context
        .publish_scalar("subject_thread_itid", json!(405), "test")
        .expect("scalar publishes");
    context
        .publish_interval("target_window", 245720189000, 246329390000, "test")
        .expect("interval publishes");

    let rendered = kat_rs_daemon::runs::render::render_template(
        "select {{ctx.subject_thread_itid}} as itid, {{ctx.target_window.start}} as start_ts, {{ctx.target_window.end}} as end_ts",
        &context,
    )
    .expect("template renders");

    assert_eq!(
        rendered,
        "select 405 as itid, 245720189000 as start_ts, 246329390000 as end_ts"
    );
}
```

- [ ] **Step 2: Run failing test**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- context_renderer_replaces_scalar_and_interval_slots --nocapture
```

Expected: FAIL because context/render modules are empty.

- [ ] **Step 3: Implement context store**

Replace `crates/kat-rs-daemon/src/runs/context.rs` with:

```rust
use std::collections::HashMap;

use serde_json::{Value, json};

use crate::error::ApiError;

#[derive(Clone, Debug)]
pub enum ContextValue {
    Scalar(Value),
    Interval { start: i64, end: i64 },
}

#[derive(Clone, Debug)]
pub struct ContextPublication {
    pub slot: String,
    pub carrier: String,
    pub value: Value,
    pub producing_step: String,
}

#[derive(Clone, Debug, Default)]
pub struct ContextStore {
    values: HashMap<String, ContextValue>,
    publications: Vec<ContextPublication>,
}

impl ContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish_scalar(
        &mut self,
        slot: &str,
        value: Value,
        producing_step: &str,
    ) -> Result<(), ApiError> {
        self.values.insert(slot.to_owned(), ContextValue::Scalar(value.clone()));
        self.publications.push(ContextPublication {
            slot: slot.to_owned(),
            carrier: "scalar".to_owned(),
            value,
            producing_step: producing_step.to_owned(),
        });
        Ok(())
    }

    pub fn publish_interval(
        &mut self,
        slot: &str,
        start: i64,
        end: i64,
        producing_step: &str,
    ) -> Result<(), ApiError> {
        self.values
            .insert(slot.to_owned(), ContextValue::Interval { start, end });
        self.publications.push(ContextPublication {
            slot: slot.to_owned(),
            carrier: "interval".to_owned(),
            value: json!({ "start": start, "end": end }),
            producing_step: producing_step.to_owned(),
        });
        Ok(())
    }

    pub fn value(&self, slot: &str) -> Result<&ContextValue, ApiError> {
        self.values
            .get(slot)
            .ok_or_else(|| ApiError::validation(format!("context slot is not published: {slot}")))
    }

    pub fn publications(&self) -> &[ContextPublication] {
        &self.publications
    }
}
```

- [ ] **Step 4: Implement renderer**

Replace `crates/kat-rs-daemon/src/runs/render.rs` with:

```rust
use regex::Regex;
use serde_json::Value;

use crate::error::ApiError;

use super::context::{ContextStore, ContextValue};

pub fn render_template(input: &str, context: &ContextStore) -> Result<String, ApiError> {
    let pattern = Regex::new(r"\{\{ctx\.([A-Za-z0-9_]+)(?:\.(start|end))?\}\}")
        .map_err(|error| ApiError::internal(format!("invalid context regex: {error}")))?;
    let mut rendered = String::with_capacity(input.len());
    let mut last = 0;

    for captures in pattern.captures_iter(input) {
        let whole = captures.get(0).expect("whole match exists");
        rendered.push_str(&input[last..whole.start()]);
        let slot = captures.get(1).expect("slot capture exists").as_str();
        let field = captures.get(2).map(|field| field.as_str());
        rendered.push_str(&render_value(slot, field, context)?);
        last = whole.end();
    }
    rendered.push_str(&input[last..]);

    Ok(rendered)
}

fn render_value(
    slot: &str,
    field: Option<&str>,
    context: &ContextStore,
) -> Result<String, ApiError> {
    match (context.value(slot)?, field) {
        (ContextValue::Scalar(value), None) => Ok(render_scalar(value)),
        (ContextValue::Interval { start, .. }, Some("start")) => Ok(start.to_string()),
        (ContextValue::Interval { end, .. }, Some("end")) => Ok(end.to_string()),
        (ContextValue::Scalar(_), Some(field)) => Err(ApiError::validation(format!(
            "context scalar slot {slot} does not have field {field}"
        ))),
        (ContextValue::Interval { .. }, None) => Err(ApiError::validation(format!(
            "context interval slot {slot} must reference start or end"
        ))),
        (ContextValue::Interval { .. }, Some(field)) => Err(ApiError::validation(format!(
            "context interval slot {slot} does not have field {field}"
        ))),
    }
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.replace('\'', "''"),
        Value::Array(_) | Value::Object(_) => value.to_string().replace('\'', "''"),
    }
}
```

- [ ] **Step 5: Implement resource structs and loader**

Replace `crates/kat-rs-daemon/src/runs/resources.rs` with:

```rust
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::ApiError;

#[derive(Clone, Debug)]
pub struct ResourceRoot {
    root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LoadedYaml<T> {
    pub path: PathBuf,
    pub digest: String,
    pub value: T,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub packs: BTreeMap<String, ManifestPack>,
    #[serde(default)]
    pub resources: ManifestResources,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestPack {
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ManifestResources {
    #[serde(default)]
    pub flows: BTreeMap<String, ManifestResource>,
    #[serde(default)]
    pub grep: BTreeMap<String, ManifestResource>,
    #[serde(default)]
    pub query: BTreeMap<String, ManifestResource>,
    #[serde(default)]
    pub summaries: BTreeMap<String, ManifestResource>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestResource {
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pack {
    pub pack: PackIdentity,
    pub inputs: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub imports: PackImports,
    pub entry_flow: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackIdentity {
    pub id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PackImports {
    #[serde(default)]
    pub flows: BTreeMap<String, String>,
    #[serde(default)]
    pub summaries: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Flow {
    pub id: String,
    #[serde(default)]
    pub constants: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FlowStep {
    pub id: String,
    pub uses: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub state: Option<serde_json::Value>,
    #[serde(default)]
    pub max_iterations: Option<serde_json::Value>,
    #[serde(default)]
    pub accumulators: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub body: Vec<FlowStep>,
    #[serde(default)]
    pub next_state: Option<serde_json::Value>,
    #[serde(default)]
    pub when: Option<serde_json::Value>,
    #[serde(default)]
    pub then: Vec<FlowStep>,
    #[serde(default, rename = "else")]
    pub else_steps: Vec<FlowStep>,
}

impl ResourceRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn load_manifest(&self) -> Result<LoadedYaml<Manifest>, ApiError> {
        self.load_yaml(self.root.join("manifest.yaml"))
    }

    pub fn load_pack(&self, manifest: &Manifest, pack_ref: &str) -> Result<LoadedYaml<Pack>, ApiError> {
        let pack = manifest
            .packs
            .get(pack_ref)
            .ok_or_else(|| ApiError::validation(format!("pack ref not found: {pack_ref}")))?;
        self.load_yaml(self.root.join(&pack.path))
    }

    pub fn load_flow_by_path(&self, path: impl AsRef<Path>) -> Result<LoadedYaml<Flow>, ApiError> {
        self.load_yaml(self.root.join(path))
    }

    fn load_yaml<T: for<'de> Deserialize<'de>>(&self, path: PathBuf) -> Result<LoadedYaml<T>, ApiError> {
        let bytes = fs::read(&path).map_err(|error| {
            ApiError::validation(format!("failed to read resource {}: {error}", path.display()))
        })?;
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
        let value = serde_yaml::from_slice(&bytes).map_err(|error| {
            ApiError::validation(format!("failed to parse resource {}: {error}", path.display()))
        })?;
        Ok(LoadedYaml { path, digest, value })
    }
}
```

- [ ] **Step 6: Run context/render test**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- context_renderer_replaces_scalar_and_interval_slots --nocapture
```

Expected: PASS.

- [ ] **Step 7: Run daemon tests**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract
cargo test -p kat-rs-daemon
```

Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add crates/kat-rs-daemon/src/runs/context.rs crates/kat-rs-daemon/src/runs/render.rs crates/kat-rs-daemon/src/runs/resources.rs crates/kat-rs-daemon/tests/runs_contract.rs
git commit -m "feat: load pack resources and render context"
```

---

### Task 7: Implement Run Operators and End-to-End Pack Execution

**Files:**
- Modify: `crates/kat-rs-daemon/src/runs/operators.rs`
- Modify: `crates/kat-rs-daemon/src/runs/service.rs`
- Modify: `crates/kat-rs-daemon/src/runs/model.rs`
- Modify: `resources/openharmony/query/candidate_path_edges.yaml`
- Test: `crates/kat-rs-daemon/tests/runs_contract.rs`

- [ ] **Step 1: Fix pack resource contract for `instant.rowid`**

Modify `resources/openharmony/query/candidate_path_edges.yaml`:

```yaml
    instant:
      columns: [rowid, ts, name, ref, wakeup_from, ref_type]
```

Keep the rest of the file unchanged.

- [ ] **Step 2: Add end-to-end run test**

Append to `crates/kat-rs-daemon/tests/runs_contract.rs`:

```rust
#[tokio::test]
async fn runs_openharmony_critical_task_pack_on_materialized_sqlite_dataset() {
    let workspace = std::env::current_dir().expect("current dir");
    let sqlite_path = workspace.join("test").join("test.db");
    assert!(sqlite_path.exists(), "expected fixture {}", sqlite_path.display());
    let temp = tempfile::tempdir().expect("tempdir is created");
    let datasets_root = temp.path().join("datasets");
    let app = kat_rs_daemon::router(kat_rs_daemon::AppState::new_for_tests());

    let create_dataset = request_json(
        app.clone(),
        "POST",
        "/v1/datasets",
        Some(json!({
            "dataset": {
                "name": "openharmony-test",
                "directory": datasets_root.to_string_lossy(),
            },
            "input": {
                "source": "SQLITE",
                "file": sqlite_path.to_string_lossy(),
            }
        })),
    )
    .await;
    assert_eq!(create_dataset.status, StatusCode::CREATED, "{:?}", create_dataset.body);

    let create_run = request_json(
        app.clone(),
        "POST",
        "/v1/runs",
        Some(json!({
            "packRef": "openharmony.critical_task_extraction",
            "dataset": {
                "name": "openharmony-test",
                "directory": datasets_root.to_string_lossy(),
            },
            "inputs": {
                "process_name_pattern": "(^|\\.)tencent\\.wechat$|^com\\.tencent\\.wechat$",
                "start_marker_pattern": "HandleLaunchAbility.*com\\.tencent\\.wechat",
                "end_marker_pattern": "UIVsyncTask.*firstDrawFrame\\s*[:=]\\s*1"
            }
        })),
    )
    .await;
    assert_eq!(create_run.status, StatusCode::OK, "{:?}", create_run.body);
    assert_eq!(create_run.body["data"]["status"], "COMPLETED");
    assert_eq!(create_run.body["data"]["packRef"], "openharmony.critical_task_extraction");
    assert_eq!(create_run.body["data"]["evidenceCount"], 2);
    let run_id = create_run.body["data"]["runId"].as_str().expect("run id");

    let evidence = request_json(
        app.clone(),
        "GET",
        &format!("/v1/runs/{run_id}/evidence"),
        None,
    )
    .await;
    assert_eq!(evidence.status, StatusCode::OK, "{:?}", evidence.body);
    let evidence_records = evidence.body["data"]["evidence"].as_array().expect("evidence array");
    let task_shape = evidence_records
        .iter()
        .find(|record| record["id"] == "critical_task_shape")
        .expect("critical task evidence");
    assert_eq!(task_shape["metrics"]["path_edge_count"], 8);
    assert_eq!(task_shape["metrics"]["path_step_count"], 8);
    assert_eq!(task_shape["metrics"]["task_count"], 8);
    assert_eq!(task_shape["metrics"]["total_ranked_duration_ns"], 3544401000i64);

    let brief = request_json(
        app,
        "GET",
        &format!("/v1/runs/{run_id}/brief"),
        None,
    )
    .await;
    assert_eq!(brief.status, StatusCode::OK, "{:?}", brief.body);
    let sections = brief.body["data"]["sections"].as_array().expect("brief sections");
    assert!(sections.iter().any(|section| section["id"] == "critical_tasks"));
}
```

- [ ] **Step 3: Run failing end-to-end test**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- runs_openharmony_critical_task_pack_on_materialized_sqlite_dataset --nocapture
```

Expected: FAIL because run service still returns placeholder failed run.

- [ ] **Step 4: Implement operator execution types**

Replace `crates/kat-rs-daemon/src/runs/operators.rs` with a module that exposes:

```rust
use arrow_array::RecordBatch;
use regex::Regex;
use serde_json::{Value, json};

use crate::error::ApiError;
use kat_rs_datasource::TraceDatasource;

use super::{
    context::ContextStore,
    model::RunStepRecord,
    render::render_template,
    resources::FlowStep,
};

pub struct ExecutionState {
    pub datasource: TraceDatasource,
    pub context: ContextStore,
    pub steps: Vec<RunStepRecord>,
    pub diagnostics: Vec<Value>,
    pub evidence: Vec<Value>,
    pub brief_sections: Vec<Value>,
}

impl ExecutionState {
    pub fn new(datasource: TraceDatasource, context: ContextStore) -> Self {
        Self {
            datasource,
            context,
            steps: Vec::new(),
            diagnostics: Vec::new(),
            evidence: Vec::new(),
            brief_sections: Vec::new(),
        }
    }

    pub fn record_step(&mut self, id: &str, uses: &str, output: Option<String>, row_count: Option<usize>) {
        self.steps.push(RunStepRecord {
            id: id.to_owned(),
            uses: uses.to_owned(),
            status: "COMPLETED".to_owned(),
            output,
            row_count,
        });
    }
}

pub async fn execute_query_step(
    state: &mut ExecutionState,
    step: &FlowStep,
    sql: &str,
    output_table: &str,
) -> Result<(), ApiError> {
    let sql = render_template(sql, &state.context)?;
    let batches = state
        .datasource
        .query_batches(&sql)
        .await
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    let row_count = batches.iter().map(RecordBatch::num_rows).sum::<usize>();
    state
        .datasource
        .register_record_batches(output_table, batches)
        .map_err(|error| ApiError::query_failed(format!("{error:#}")))?;
    state.record_step(&step.id, "query", Some(output_table.to_owned()), Some(row_count));
    Ok(())
}
```

Then add concrete implementations in the same file for:

- `execute_grep_step`: build a DataFusion SQL projection for source rows, collect JSON rows, filter with `regex::Regex`, build a `RecordBatch` from filtered rows, register `output.table`, and publish the first row columns described by `context.publishes`.
- `execute_branch_step`: evaluate `when.row_count.table equals 0`, then execute either `then` or `else_steps`.
- `execute_loop_step`: execute the static `body` up to `max_iterations`, append `selected_path_edges` into `path_edges`, append `path_step_rows` into `path_steps`, and stop when `next_anchor_rows` is empty.
- `execute_summaries_step`: compute evidence records from `summary.evidence`, using aggregate SQL for metrics and query SQL for refs.
- `build_brief_sections`: read `brief.yaml` sections and query each public artifact with projection, ordering, and limit.

Use these exact helper SQL patterns:

```rust
fn row_count_sql(table: &str) -> String {
    format!("select count(*) as row_count from {table}")
}

fn append_table_sql(target: &str, source: &str) -> String {
    format!("select * from {target} union all select * from {source}")
}
```

For accumulator append, query `append_table_sql`, register the result back under the accumulator name, and record one step with `uses = "accumulate"`.

- [ ] **Step 5: Replace placeholder run service with real execution**

Modify `crates/kat-rs-daemon/src/runs/service.rs` so `create_placeholder` becomes `create` and:

1. Resolves dataset path from `DatasetDto.path`.
2. Opens `TraceDatasource::from_dataset`.
3. Loads resources from `ResourceRoot::new("resources")`.
4. Publishes request inputs and flow constants into `ContextStore`.
5. Executes the entry flow steps.
6. Builds evidence and brief.
7. Stores a `RunRecord` with `status = "COMPLETED"`.

Use this function signature:

```rust
pub async fn create(
    &self,
    request: CreateRunRequest,
    dataset: DatasetDto,
) -> Result<Arc<RunRecord>, ApiError>
```

Update `routes/runs.rs` to call:

```rust
let run = state.run_service.create(request, dataset).await?;
```

- [ ] **Step 6: Run end-to-end test**

Run:

```powershell
cargo test -p kat-rs-daemon --test runs_contract -- runs_openharmony_critical_task_pack_on_materialized_sqlite_dataset --nocapture
```

Expected: PASS with `status = COMPLETED`, `task_count = 8`, and `total_ranked_duration_ns = 3544401000`.

- [ ] **Step 7: Run daemon tests**

Run:

```powershell
cargo test -p kat-rs-daemon
```

Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add crates/kat-rs-daemon/src/runs/operators.rs crates/kat-rs-daemon/src/runs/service.rs crates/kat-rs-daemon/src/runs/model.rs crates/kat-rs-daemon/src/routes/runs.rs crates/kat-rs-daemon/tests/runs_contract.rs resources/openharmony/query/candidate_path_edges.yaml
git commit -m "feat: execute critical task pack runs"
```

---

### Task 8: Final Verification and Documentation Update

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add README smoke commands**

Add a short section to `README.md` near the existing dataset query section:

```markdown
### OpenHarmony SQLite MVP run

The MVP run path imports an OpenHarmony SQLite export into the local Parquet dataset layout, then executes a pack through `/v1/runs`.

```bash
curl -sS -X POST http://127.0.0.1:3030/v1/datasets \
  -H 'content-type: application/json' \
  -d '{
    "dataset": {
      "name": "openharmony-test",
      "directory": "D:/work/kat_rs/0706/kat-rs/test/datasets"
    },
    "input": {
      "source": "SQLITE",
      "file": "D:/work/kat_rs/0706/kat-rs/test/test.db"
    }
  }'

curl -sS -X POST http://127.0.0.1:3030/v1/runs \
  -H 'content-type: application/json' \
  -d '{
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
  }'
```
```

- [ ] **Step 2: Run full verification**

Run:

```powershell
cargo fmt --all -- --check
cargo test -p kat-rs-datasource
cargo test -p kat-rs-daemon
cargo test -p kat-rs-cli
```

Expected: all commands PASS.

- [ ] **Step 3: Run OpenAPI smoke**

Run:

```powershell
cargo run -p kat-rs-cli -- openapi
```

Expected: JSON output includes `/v1/runs`, `/v1/runs/{runId}`, `/v1/runs/{runId}/evidence`, and `/v1/runs/{runId}/brief`.

- [ ] **Step 4: Commit**

```powershell
git add README.md
git commit -m "docs: document sqlite mvp run smoke path"
```

- [ ] **Step 5: Final status check**

Run:

```powershell
git status --short
```

Expected: no modified tracked files. Untracked user-provided fixtures may remain if they were untracked before implementation.

---

## Self-Review

Spec coverage:

- SQLite materialization belongs to `kat-rs-datasource`: covered by Tasks 2 and 3.
- `POST /v1/datasets` remains the clean daemon entry point: covered by Task 3.
- `/v1/runs` accepts dataset ref, pack ref, and inputs, not SQLite path: covered by Tasks 5 and 7.
- Run execution uses Parquet catalog through DataFusion: covered by Tasks 4 and 7.
- Pack runtime supports grep/query/branch/loop/summaries only for the demo pack: covered by Task 7.
- Evidence and brief endpoints: covered by Tasks 5 and 7.
- `instant.rowid` contract: covered by Tasks 2 and 7.
- Expected `test/test.db` metrics: covered by Task 7.

Placeholder scan:

- The plan contains no placeholder markers or unspecified validation steps.
- The only large implementation area is Task 7; it names the exact operator functions and required behavior because this is the MVP core.

Type consistency:

- Run DTO names are introduced in Task 5 and reused in routes/OpenAPI.
- `TraceDatasource::query_batches` and `register_record_batches` are introduced in Task 4 before Task 7 uses them.
- `ContextStore` and `render_template` are introduced in Task 6 before Task 7 uses them.
