use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    analysis::derived::DerivedRunner,
    pack::{
        LoadedPack, PackManifest, load_pack,
        spec::{InputTables, TransformOutputSpec, TransformSafetySpec, TransformSpec},
    },
};
use rusqlite::Connection;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};
use tempfile::{TempDir, tempdir};

#[test]
fn derived_runner_materializes_requested_transforms_once() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw");
    conn.execute(
        "CREATE TABLE thread_state (itid INTEGER, ts INTEGER, dur INTEGER, state TEXT)",
        [],
    )
    .expect("thread_state");
    conn.execute("INSERT INTO thread_state VALUES (7, 10, 5, 'R')", [])
        .expect("row");
    drop(conn);

    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(
            &mut adapter,
            "thread_state_segments",
            &json!({ "itid": 7 }),
            &json!({}),
        )
        .expect("first materialization");
    runner
        .ensure_table(
            &mut adapter,
            "thread_state_segments",
            &json!({ "itid": 7 }),
            &json!({}),
        )
        .expect("second materialization is no-op");

    assert!(
        adapter
            .table_exists("thread_state_segments")
            .expect("table exists")
    );
    let rows = adapter
        .query_json("SELECT itid, start_ts, end_ts, state_class FROM thread_state_segments")
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state_class"], "runnable");
}

#[test]
fn derived_runner_rejects_duplicate_output_table_producers() {
    let dir = tempdir().expect("tempdir");
    let pack = synthetic_pack(
        dir.path().to_path_buf(),
        vec![
            sql_transform("first", "raw_input", "same_output", "first.sql"),
            sql_transform("second", "raw_input", "same_output", "second.sql"),
        ],
    );

    let error = DerivedRunner::new(&pack).expect_err("duplicate output should fail");

    let message = error.to_string();
    assert!(message.contains("duplicate transform output table `same_output`"));
    assert!(message.contains("first"));
    assert!(message.contains("second"));
}

#[test]
fn derived_runner_materializes_dependency_chain() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_input", "value INTEGER", "41");
    fixture.write_sql("a.sql", "SELECT value + 1 AS value FROM raw_input");
    fixture.write_sql("b.sql", "SELECT value + 1 AS value FROM intermediate");
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![
            sql_transform("make_intermediate", "raw_input", "intermediate", "a.sql"),
            sql_transform("make_final", "intermediate", "final_table", "b.sql"),
        ],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(&mut adapter, "final_table", &json!({}), &json!({}))
        .expect("materialize chain");

    assert!(adapter.table_exists("intermediate").expect("intermediate"));
    assert!(adapter.table_exists("final_table").expect("final_table"));
    let rows = adapter
        .query_json("SELECT value FROM final_table")
        .expect("rows");
    assert_eq!(rows[0]["value"], 43);
}

#[test]
fn derived_runner_reused_with_second_adapter_materializes_again() {
    let first = SqlFixture::new();
    let second = SqlFixture::new();
    first.create_raw_table("raw_input", "value INTEGER", "10");
    second.create_raw_table("raw_input", "value INTEGER", "20");
    first.write_sql("derived.sql", "SELECT value + 1 AS value FROM raw_input");
    let pack = synthetic_pack(
        first.pack_root(),
        vec![sql_transform(
            "make_derived",
            "raw_input",
            "derived_table",
            "derived.sql",
        )],
    );
    let mut first_adapter = first.adapter();
    let mut second_adapter = second.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(&mut first_adapter, "derived_table", &json!({}), &json!({}))
        .expect("first adapter materialization");
    runner
        .ensure_table(&mut second_adapter, "derived_table", &json!({}), &json!({}))
        .expect("second adapter materialization");

    let rows = second_adapter
        .query_json("SELECT value FROM derived_table")
        .expect("second rows");
    assert_eq!(rows[0]["value"], 21);
}

#[test]
fn derived_runner_reused_with_second_adapter_reports_output_collision() {
    let first = SqlFixture::new();
    let second = SqlFixture::new();
    first.create_raw_table("raw_input", "value INTEGER", "10");
    second.create_raw_table("raw_input", "value INTEGER", "20");
    second.create_raw_table("derived_table", "value INTEGER", "99");
    first.write_sql("derived.sql", "SELECT value + 1 AS value FROM raw_input");
    let pack = synthetic_pack(
        first.pack_root(),
        vec![sql_transform(
            "make_derived",
            "raw_input",
            "derived_table",
            "derived.sql",
        )],
    );
    let mut first_adapter = first.adapter();
    let mut second_adapter = second.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(&mut first_adapter, "derived_table", &json!({}), &json!({}))
        .expect("first adapter materialization");
    let error = runner
        .ensure_table(&mut second_adapter, "derived_table", &json!({}), &json!({}))
        .expect_err("second adapter collision");

    let message = error.to_string();
    assert!(message.contains("derived table `derived_table` already exists"));
    assert!(message.contains("make_derived"));
    assert!(message.contains("not materialized by this runner"));
}

#[test]
fn derived_runner_reports_existing_table_collision_for_transform_output() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_input", "value INTEGER", "10");
    fixture.create_raw_table("derived_table", "value INTEGER", "99");
    fixture.write_sql("derived.sql", "SELECT value + 1 AS value FROM raw_input");
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![sql_transform(
            "make_derived",
            "raw_input",
            "derived_table",
            "derived.sql",
        )],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    let error = runner
        .ensure_table(&mut adapter, "derived_table", &json!({}), &json!({}))
        .expect_err("existing transform output should collide");

    let message = error.to_string();
    assert!(message.contains("derived table `derived_table` already exists"));
    assert!(message.contains("make_derived"));
    assert!(message.contains("not materialized by this runner"));
}

#[test]
fn derived_runner_reports_dependency_cycles() {
    let fixture = SqlFixture::new();
    fixture.write_sql("a.sql", "SELECT value FROM table_b");
    fixture.write_sql("b.sql", "SELECT value FROM table_a");
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![
            sql_transform("make_a", "table_b", "table_a", "a.sql"),
            sql_transform("make_b", "table_a", "table_b", "b.sql"),
        ],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    let error = runner
        .ensure_table(&mut adapter, "table_a", &json!({}), &json!({}))
        .expect_err("cycle should fail");

    let message = error.to_string();
    assert!(message.contains("cycle while materializing derived table"));
    assert!(message.contains("table_a"));
}

#[test]
fn derived_runner_reports_missing_unproduced_input() {
    let fixture = SqlFixture::new();
    fixture.write_sql("derived.sql", "SELECT value FROM missing_raw");
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![sql_transform(
            "make_derived",
            "missing_raw",
            "derived_table",
            "derived.sql",
        )],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    let error = runner
        .ensure_table(&mut adapter, "derived_table", &json!({}), &json!({}))
        .expect_err("missing input should fail");

    let message = error.to_string();
    assert!(message.contains("transform `make_derived` input table `missing_raw`"));
    assert!(message.contains("not produced by a pack transform"));
}

#[test]
fn derived_runner_noops_for_existing_raw_table_without_producer() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_only", "value INTEGER", "7");
    let pack = synthetic_pack(fixture.pack_root(), Vec::new());
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(&mut adapter, "raw_only", &json!({}), &json!({}))
        .expect("existing raw table");

    let rows = adapter
        .query_json("SELECT value FROM raw_only")
        .expect("rows");
    assert_eq!(rows[0]["value"], 7);
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

struct SqlFixture {
    dir: TempDir,
    raw_db: PathBuf,
    scratch_db: PathBuf,
}

impl SqlFixture {
    fn new() -> Self {
        let dir = tempdir().expect("tempdir");
        let raw_db = dir.path().join("raw.db");
        let scratch_db = dir.path().join("scratch.db");
        Connection::open(&raw_db).expect("raw");
        Self {
            dir,
            raw_db,
            scratch_db,
        }
    }

    fn pack_root(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    fn write_sql(&self, path: &str, sql: &str) {
        fs::write(self.dir.path().join(path), sql).expect("write sql");
    }

    fn create_raw_table(&self, table: &str, columns: &str, values: &str) {
        let conn = Connection::open(&self.raw_db).expect("raw");
        conn.execute(&format!("CREATE TABLE {table} ({columns})"), [])
            .expect("create raw table");
        conn.execute(&format!("INSERT INTO {table} VALUES ({values})"), [])
            .expect("insert raw row");
    }

    fn adapter(&self) -> SQLiteDatasetAdapter {
        SQLiteDatasetAdapter::open(&self.raw_db, &self.scratch_db).expect("adapter")
    }
}

fn synthetic_pack(root: PathBuf, transforms: Vec<TransformSpec>) -> LoadedPack {
    LoadedPack {
        root,
        manifest: PackManifest {
            id: "synthetic".to_string(),
            name: None,
            schemas: Vec::new(),
            derived: Vec::new(),
            queries: Vec::new(),
            analyses: Vec::new(),
            rules: Vec::new(),
        },
        transforms,
        analyses: Vec::new(),
        rule_sets: Vec::new(),
    }
}

fn sql_transform(id: &str, input: &str, output: &str, sql: &str) -> TransformSpec {
    TransformSpec {
        id: id.to_string(),
        kind: "sql.view".to_string(),
        inputs: InputTables::List(vec![input.to_string()]),
        sql: Some(PathBuf::from(sql)),
        params: BTreeMap::new(),
        bind: BTreeMap::new(),
        where_: BTreeMap::new(),
        source: None,
        fields: BTreeMap::new(),
        joins: BTreeMap::new(),
        filters: BTreeMap::new(),
        output: TransformOutputSpec {
            table: output.to_string(),
            schema: "synthetic".to_string(),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: vec![input.to_string()],
        },
    }
}
