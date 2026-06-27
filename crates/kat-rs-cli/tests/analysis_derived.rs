use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    analysis::derived::DerivedRunner,
    pack::{
        LoadedPack, PackManifest, load_pack,
        spec::{
            AnalysisStepSpec, BindingExpr, GraphProviderSpec, GraphValueSpec, InputTables,
            MarkerSourceSpec, PredicateSpec, TransformOutputSpec, TransformSafetySpec,
            TransformSpec,
        },
    },
};
use rusqlite::Connection;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};
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
fn derived_runner_same_adapter_same_params_noops() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_input", "value INTEGER", "10");
    fixture.write_sql(
        "derived.sql",
        "SELECT value + ${delta} AS value FROM raw_input",
    );
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
    let params = json!({ "delta": 5 });

    runner
        .ensure_table(&mut adapter, "derived_table", &params, &json!({}))
        .expect("first materialization");
    runner
        .ensure_table(&mut adapter, "derived_table", &params, &json!({}))
        .expect("same params no-op");

    let rows = adapter
        .query_json("SELECT value FROM derived_table")
        .expect("rows");
    assert_eq!(rows[0]["value"], 15);
}

#[test]
fn derived_runner_same_adapter_different_params_errors() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_input", "value INTEGER", "10");
    fixture.write_sql(
        "derived.sql",
        "SELECT value + ${delta} AS value FROM raw_input",
    );
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

    runner
        .ensure_table(
            &mut adapter,
            "derived_table",
            &json!({ "delta": 5 }),
            &json!({}),
        )
        .expect("first materialization");
    let error = runner
        .ensure_table(
            &mut adapter,
            "derived_table",
            &json!({ "delta": 6 }),
            &json!({}),
        )
        .expect_err("different params should fail");

    let message = error.to_string();
    assert!(message.contains("derived table `derived_table` was already materialized"));
    assert!(message.contains("different params/state"));
}

#[test]
fn derived_runner_ignores_state_changes_for_state_independent_transform() {
    let fixture = SqlFixture::new();
    fixture.create_raw_table("raw_input", "value INTEGER", "10");
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

    runner
        .ensure_table(
            &mut adapter,
            "derived_table",
            &json!({}),
            &json!({ "root": { "process_name": ".first" } }),
        )
        .expect("first materialization");
    runner
        .ensure_table(
            &mut adapter,
            "derived_table",
            &json!({}),
            &json!({ "root": { "process_name": ".second" } }),
        )
        .expect("state-independent transform ignores later state changes");

    let rows = adapter
        .query_json("SELECT value FROM derived_table")
        .expect("rows");
    assert_eq!(rows[0]["value"], 11);
}

#[test]
fn derived_runner_rejects_state_changes_for_state_dependent_transform() {
    let fixture = SqlFixture::new();
    let conn = Connection::open(&fixture.raw_db).expect("raw");
    conn.execute_batch(
        "
        CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT);
        CREATE TABLE thread (
            itid INTEGER,
            tid INTEGER,
            ipid INTEGER,
            thread_name TEXT,
            is_main_thread INTEGER
        );
        CREATE TABLE callstack (
            id INTEGER,
            callid INTEGER,
            parent_id INTEGER,
            name TEXT,
            ts INTEGER,
            dur INTEGER
        );

        INSERT INTO process VALUES (7, 1001, '.first');
        INSERT INTO thread VALUES (405, 1001, 7, 'main', 1);
        INSERT INTO callstack VALUES (
            30754,
            405,
            NULL,
            'firstDrawFrame:1 [vsyncID:3269] [layoutMeasureDurationStartTimestamp:1000] [layoutMeasureDurationEndTimestamp:3000]',
            900,
            2200
        );
        ",
    )
    .expect("raw fixture");
    drop(conn);
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![state_filtered_marker_transform(
            "state_filtered_window",
            "state_filtered_window",
        )],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(
            &mut adapter,
            "state_filtered_window",
            &json!({ "marker": "firstDrawFrame:1" }),
            &json!({ "root": { "process_name": ".first" } }),
        )
        .expect("first materialization");
    let error = runner
        .ensure_table(
            &mut adapter,
            "state_filtered_window",
            &json!({ "marker": "firstDrawFrame:1" }),
            &json!({ "root": { "process_name": ".second" } }),
        )
        .expect_err("state-dependent transform rejects later state changes");

    let message = error.to_string();
    assert!(message.contains("derived table `state_filtered_window`"));
    assert!(message.contains("different params/state"));
}

#[test]
fn derived_runner_rejects_state_changes_for_transitively_state_dependent_transform() {
    let fixture = SqlFixture::new();
    fixture.write_sql(
        "parent.sql",
        "SELECT callstack_id, itid, process_name FROM state_child",
    );
    let conn = Connection::open(&fixture.raw_db).expect("raw");
    conn.execute_batch(
        "
        CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT);
        CREATE TABLE thread (
            itid INTEGER,
            tid INTEGER,
            ipid INTEGER,
            thread_name TEXT,
            is_main_thread INTEGER
        );
        CREATE TABLE callstack (
            id INTEGER,
            callid INTEGER,
            parent_id INTEGER,
            name TEXT,
            ts INTEGER,
            dur INTEGER
        );

        INSERT INTO process VALUES (7, 1001, '.first');
        INSERT INTO thread VALUES (405, 1001, 7, 'main', 1);
        INSERT INTO callstack VALUES (
            30754,
            405,
            NULL,
            'firstDrawFrame:1 [vsyncID:3269] [layoutMeasureDurationStartTimestamp:1000] [layoutMeasureDurationEndTimestamp:3000]',
            900,
            2200
        );
        ",
    )
    .expect("raw fixture");
    drop(conn);
    let pack = synthetic_pack(
        fixture.pack_root(),
        vec![
            state_filtered_marker_transform("state_child", "state_child"),
            sql_transform(
                "state_parent_from_child",
                "state_child",
                "state_parent",
                "parent.sql",
            ),
        ],
    );
    let mut adapter = fixture.adapter();
    let mut runner = DerivedRunner::new(&pack).expect("runner");

    runner
        .ensure_table(
            &mut adapter,
            "state_parent",
            &json!({ "marker": "firstDrawFrame:1" }),
            &json!({ "root": { "process_name": ".first" } }),
        )
        .expect("first materialization");
    let error = runner
        .ensure_table(
            &mut adapter,
            "state_parent",
            &json!({ "marker": "firstDrawFrame:1" }),
            &json!({ "root": { "process_name": ".second" } }),
        )
        .expect_err("transitively state-dependent transform rejects later state changes");

    let message = error.to_string();
    assert!(message.contains("derived table `state_parent`"));
    assert!(message.contains("different params/state"));
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

#[test]
fn openharmony_callstack_self_time_ignores_overlapping_non_root_subtree_spans() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");
    let conn = Connection::open(&raw_db).expect("raw");
    conn.execute_batch(
        "
        CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT);
        CREATE TABLE thread (
            itid INTEGER,
            tid INTEGER,
            ipid INTEGER,
            thread_name TEXT,
            is_main_thread INTEGER
        );
        CREATE TABLE callstack (
            id INTEGER,
            callid INTEGER,
            parent_id INTEGER,
            name TEXT,
            ts INTEGER,
            dur INTEGER
        );

        INSERT INTO process VALUES (89, 15040, '.tencent.wechat');
        INSERT INTO thread VALUES (405, 15040, 89, '.tencent.wechat', 1);
        INSERT INTO callstack VALUES
            (20000, 405, 405, 'H:APP_COMPONENT_LOAD', 50, 1000),
            (30493, 405, NULL, 'H:UIVsyncTask[timestamp:90][vsyncID:3269]|M0539', 90, 120),
            (30754, 405, 30493, 'H:UIVsyncTask[timestamp:90][vsyncID:3269][layoutMeasureDurationStartTimestamp:100][layoutMeasureDurationEndTimestamp:200][firstDrawFrame:1]|M0539', 100, 100),
            (30544, 405, 30754, 'CreateImagePixelMap resource:///1140850711.png', 120, 70);
        ",
    )
    .expect("fixture");
    drop(conn);

    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let mut runner = DerivedRunner::new(&pack).expect("runner");
    runner
        .ensure_table(
            &mut adapter,
            "callstack_self_time",
            &json!({
                "target_process": ".tencent.wechat",
                "marker": "firstDrawFrame:1"
            }),
            &json!({}),
        )
        .expect("callstack self time");

    let rows = adapter
        .query_json("SELECT name FROM callstack_self_time ORDER BY exclusive_rank LIMIT 1")
        .expect("self time rows");
    assert_eq!(
        rows[0]["name"],
        "CreateImagePixelMap resource:///1140850711.png"
    );
}

#[test]
fn openharmony_pack_declares_critical_path_derived_tables() {
    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let output_tables = pack
        .transforms
        .iter()
        .map(|transform| transform.output.table.as_str())
        .collect::<BTreeSet<_>>();

    for table in [
        "first_draw_window",
        "thread_state_profile",
        "callstack_overlap_window",
        "callstack_self_time",
        "frame_slice_link",
        "render_service_context",
        "io_sample_overlap",
        "wakeup_edges",
    ] {
        assert!(
            output_tables.contains(table),
            "missing derived table {table}"
        );
    }

    let analysis = pack
        .analyses
        .iter()
        .find(|analysis| analysis.id == "openharmony.critical_path")
        .expect("critical path analysis");
    assert!(
        analysis
            .requires
            .derived
            .contains(&"first_draw_window".to_string())
    );
    assert!(
        analysis
            .requires
            .derived
            .contains(&"callstack_self_time".to_string())
    );
}

#[test]
fn openharmony_critical_path_edge_providers_reference_declared_columns() {
    let pack = load_pack(workspace_root().join("packs/openharmony-core")).expect("pack");
    let analysis = pack
        .analyses
        .iter()
        .find(|analysis| analysis.id == "openharmony.critical_path")
        .expect("critical path analysis");

    let temporal_graph_walk = analysis.steps.iter().find_map(|step| match step {
        AnalysisStepSpec::TemporalGraphWalk(step) => Some(step),
        _ => None,
    });

    if let Some(graph_walk) = temporal_graph_walk {
        for provider in &graph_walk.edge_providers {
            let columns = sql_transform_output_columns(&pack, &provider.table);
            for field in provider.when.keys() {
                assert!(
                    columns.contains(field),
                    "provider `{}` table `{}` is missing when field `{}`; columns: {:?}",
                    provider.id,
                    provider.table,
                    field,
                    columns
                );
            }

            for field in [
                provider.emit.target.itid.as_deref(),
                provider.emit.target.start_ts.as_deref(),
                provider.emit.target.end_ts.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                assert!(
                    columns.contains(field),
                    "provider `{}` table `{}` is missing target field `{}`; columns: {:?}",
                    provider.id,
                    provider.table,
                    field,
                    columns
                );
            }

            for (fact_key, fact) in &provider.emit.facts {
                let table = fact.table.as_deref().unwrap_or(&provider.table);
                let columns = sql_transform_output_columns(&pack, table);
                if let Some(field) = &fact.field {
                    assert!(
                        columns.contains(field),
                        "provider `{}` fact `{}` table `{}` is missing field `{}`; columns: {:?}",
                        provider.id,
                        fact_key,
                        table,
                        field,
                        columns
                    );
                }
                for field in fact.row.where_.keys() {
                    assert!(
                        columns.contains(field),
                        "provider `{}` fact `{}` table `{}` is missing row selector field `{}`; columns: {:?}",
                        provider.id,
                        fact_key,
                        table,
                        field,
                        columns
                    );
                }
            }
        }

        let self_execution = graph_walk
            .edge_providers
            .iter()
            .find(|provider| provider.id == "self_execution")
            .expect("self execution provider");
        assert!(
            self_execution.emit.facts.contains_key("dominantState"),
            "self_execution should configure report-visible thread state facts"
        );
        assert!(
            self_execution.emit.facts.contains_key("topSpanName"),
            "self_execution should configure report-visible callstack facts"
        );
        return;
    }

    let graph_walk = analysis
        .steps
        .iter()
        .find_map(|step| match step {
            AnalysisStepSpec::GraphWalk(step) => Some(step),
            _ => None,
        })
        .expect("graph walk step");

    for provider in &graph_walk.providers {
        assert_generic_provider_references_declared_columns(&pack, provider);
    }

    assert!(
        graph_walk
            .providers
            .iter()
            .any(|provider| provider.id == "self_execution"
                && provider.output.annotations.contains_key("dominantState")
                && provider.output.annotations.contains_key("dominantPercent")),
        "self_execution should configure report-visible thread state annotations"
    );
    assert!(
        graph_walk
            .providers
            .iter()
            .any(|provider| provider.id == "self_top_span"
                && provider.output.annotations.contains_key("topSpanName")
                && provider.output.annotations.contains_key("topSpanDurMs")),
        "self_top_span should configure report-visible callstack annotations"
    );
}

fn assert_generic_provider_references_declared_columns(
    pack: &LoadedPack,
    provider: &GraphProviderSpec,
) {
    let columns = sql_transform_output_columns(pack, &provider.input.table);

    for table in &provider.output.evidence.tables {
        let _ = sql_transform_output_columns(pack, table);
    }

    let mut fields = BTreeSet::new();
    collect_predicate_row_fields(&provider.match_, &mut fields);
    for order_by in &provider.select.order_by {
        collect_binding_row_field(&order_by.expr, &mut fields);
    }
    for annotation in provider.output.annotations.values() {
        collect_graph_value_row_fields(annotation, &mut fields);
    }

    for field in fields {
        assert!(
            columns.contains(&field),
            "provider `{}` table `{}` is missing referenced row field `{}`; columns: {:?}",
            provider.id,
            provider.input.table,
            field,
            columns
        );
    }
}

fn collect_predicate_row_fields(predicate: &PredicateSpec, fields: &mut BTreeSet<String>) {
    match predicate {
        PredicateSpec::All(predicates) | PredicateSpec::Any(predicates) => {
            for predicate in predicates {
                collect_predicate_row_fields(predicate, fields);
            }
        }
        PredicateSpec::Not(predicate) => collect_predicate_row_fields(predicate, fields),
        PredicateSpec::Eq(values)
        | PredicateSpec::Neq(values)
        | PredicateSpec::Gt(values)
        | PredicateSpec::Gte(values)
        | PredicateSpec::Lt(values)
        | PredicateSpec::Lte(values) => {
            for value in values {
                collect_binding_row_field(value, fields);
            }
        }
        PredicateSpec::Exists(value) => collect_binding_row_field(value, fields),
        PredicateSpec::TemporalPointWithin(spec) => {
            collect_binding_row_field(&spec.point, fields);
            collect_binding_row_field(&spec.window.start, fields);
            collect_binding_row_field(&spec.window.end, fields);
        }
        PredicateSpec::TemporalOverlaps(spec) => {
            collect_binding_row_field(&spec.left.start, fields);
            collect_binding_row_field(&spec.left.end, fields);
            collect_binding_row_field(&spec.right.start, fields);
            collect_binding_row_field(&spec.right.end, fields);
        }
    }
}

fn collect_graph_value_row_fields(value: &GraphValueSpec, fields: &mut BTreeSet<String>) {
    match value {
        GraphValueSpec::Scaled { value, .. } | GraphValueSpec::Value(value) => {
            collect_binding_row_field(value, fields);
        }
    }
}

fn collect_binding_row_field(value: &BindingExpr, fields: &mut BTreeSet<String>) {
    let BindingExpr::Path(path) = value else {
        return;
    };
    let Some(field) = path.strip_prefix("row.") else {
        return;
    };
    if !field.contains('.') {
        fields.insert(field.to_string());
    }
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

fn sql_transform_output_columns(pack: &LoadedPack, table: &str) -> BTreeSet<String> {
    let transform = pack
        .transforms
        .iter()
        .find(|transform| transform.output.table == table)
        .unwrap_or_else(|| panic!("missing transform for table {table}"));
    let sql_path = transform
        .sql
        .as_ref()
        .unwrap_or_else(|| panic!("transform `{}` has no SQL", transform.id));
    let sql = fs::read_to_string(pack.root.join(sql_path)).expect("read transform sql");
    sql_select_aliases(&sql)
}

fn sql_select_aliases(sql: &str) -> BTreeSet<String> {
    sql.lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_end_matches(',').trim_end_matches(';');
            let lower = trimmed.to_ascii_lowercase();
            let alias = if let Some(index) = lower.rfind(" as ") {
                trimmed[index + 4..].trim()
            } else if trimmed
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.')
            {
                trimmed.rsplit('.').next().expect("selected column")
            } else {
                return None;
            };
            alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                .then(|| alias.to_string())
        })
        .collect()
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

fn state_filtered_marker_transform(id: &str, output: &str) -> TransformSpec {
    TransformSpec {
        id: id.to_string(),
        kind: "marker.extract_bracket_fields".to_string(),
        inputs: InputTables::List(vec![
            "callstack".to_string(),
            "thread".to_string(),
            "process".to_string(),
        ]),
        sql: None,
        params: BTreeMap::new(),
        bind: BTreeMap::new(),
        where_: BTreeMap::new(),
        source: Some(MarkerSourceSpec {
            table: "callstack".to_string(),
            column: "name".to_string(),
            contains: "${params.marker}".to_string(),
        }),
        fields: BTreeMap::from([
            (
                "start_ts".to_string(),
                "layoutMeasureDurationStartTimestamp".to_string(),
            ),
            (
                "end_ts".to_string(),
                "layoutMeasureDurationEndTimestamp".to_string(),
            ),
            ("vsync_id".to_string(), "vsyncID".to_string()),
        ]),
        joins: BTreeMap::new(),
        filters: BTreeMap::from([(
            "process_name".to_string(),
            json!("${state.root.process_name}"),
        )]),
        output: TransformOutputSpec {
            table: output.to_string(),
            schema: "marker.first_draw_window.v1".to_string(),
            semantic: None,
        },
        materialize: None,
        safety: TransformSafetySpec {
            allowed_tables: vec![
                "callstack".to_string(),
                "thread".to_string(),
                "process".to_string(),
            ],
        },
    }
}
