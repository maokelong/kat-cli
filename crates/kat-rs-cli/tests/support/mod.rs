use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use kat_rs_cli::trace_runtime::pack::{
    spec::{InputTables, RuleSetSpec, TransformOutputSpec, TransformSafetySpec, TransformSpec},
    LoadedPack, PackManifest,
};
use rusqlite::Connection;
use serde_json::{json, Value};

static NEXT_TABLE_SUFFIX: AtomicUsize = AtomicUsize::new(0);

pub fn create_raw_db_with_statements(statements: &[&str]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw db");
    for statement in statements {
        conn.execute(statement, []).expect(statement);
    }
    drop(conn);
    (dir, raw_db, scratch_db)
}

pub fn empty_loaded_pack(rule_sets: Vec<RuleSetSpec>) -> LoadedPack {
    LoadedPack {
        root: PathBuf::from("fixture-pack"),
        manifest: PackManifest {
            id: "fixture-core".to_string(),
            name: None,
            schemas: Vec::new(),
            derived: Vec::new(),
            queries: Vec::new(),
            analyses: Vec::new(),
            rules: Vec::new(),
        },
        transforms: Vec::new(),
        analyses: Vec::new(),
        rule_sets,
    }
}

pub fn extractor_pack(transform_id: &str, extractor: Value) -> LoadedPack {
    empty_loaded_pack(vec![RuleSetSpec {
        rules: Default::default(),
        extractors: [(transform_id.to_string(), extractor)].into(),
    }])
}

pub fn rules_pack(rules: Vec<(&str, Value)>) -> LoadedPack {
    empty_loaded_pack(vec![RuleSetSpec {
        rules: rules
            .into_iter()
            .map(|(class, rule)| (class.to_string(), rule))
            .collect(),
        extractors: Default::default(),
    }])
}

pub fn sql_transform(
    id: &str,
    sql: &str,
    output_table: &str,
    inputs: Vec<&str>,
    allowed: Vec<&str>,
) -> TransformSpec {
    TransformSpec {
        id: id.to_string(),
        kind: "sql.view".to_string(),
        inputs: InputTables::List(inputs.into_iter().map(str::to_string).collect()),
        sql: Some(sql.into()),
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        output: TransformOutputSpec {
            table: output_table.to_string(),
            schema: format!("{id}.v1"),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: allowed.into_iter().map(str::to_string).collect(),
        },
    }
}

pub fn payload_transform(inputs: Vec<&str>, allowed: Vec<&str>) -> TransformSpec {
    TransformSpec {
        id: "window_fields".to_string(),
        kind: "payload.extract_fields".to_string(),
        inputs: InputTables::List(inputs.into_iter().map(str::to_string).collect()),
        sql: None,
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        output: TransformOutputSpec {
            table: unique_table("derived_windows"),
            schema: "window.fields.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: allowed.into_iter().map(str::to_string).collect(),
        },
    }
}

pub fn rules_transform(inputs: Vec<&str>, allowed: Vec<&str>) -> TransformSpec {
    TransformSpec {
        id: "thread_identity".to_string(),
        kind: "rules.classify".to_string(),
        inputs: InputTables::List(inputs.into_iter().map(str::to_string).collect()),
        sql: None,
        params: Default::default(),
        bind: Default::default(),
        where_: Default::default(),
        output: TransformOutputSpec {
            table: unique_table("thread_identity"),
            schema: "thread.identity.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: allowed.into_iter().map(str::to_string).collect(),
        },
    }
}

pub fn basic_payload_extractor(source_table: &str) -> Value {
    json!({
        "source_table": source_table,
        "payload_column": "marker_payload",
        "fields": {
            "start_ts": "start"
        }
    })
}

fn unique_table(prefix: &str) -> String {
    let suffix = NEXT_TABLE_SUFFIX.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{suffix}")
}
