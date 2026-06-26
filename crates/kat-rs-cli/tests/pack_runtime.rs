use std::path::{Path, PathBuf};

use kat_rs_cli::trace_runtime::{
    adapter::{DatasetAdapter, sqlite::SQLiteDatasetAdapter},
    analysis::run_store::AnalysisRunStore,
    pack::load_pack,
    transform::{
        marker::run_marker_extract_bracket_fields_transform, rules::run_rules_classify_transform,
        sql::run_sql_view_transform,
    },
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn pack_sql_transform_and_run_state_work_on_sqlite_fixture() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("raw.db");
    let scratch_db = dir.path().join("scratch.db");

    let pack = load_pack(fixture_pack_root()).expect("pack loads");
    create_fixture_db(&raw_db, &pack);
    let analysis_id = &pack.analyses.first().expect("analysis spec").id;
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    let params = json!({
        "itid": 7,
        "marker": "firstDrawFrame:1",
        "target_process": ".tencent.wechat"
    });
    for transform in &pack.transforms {
        match transform.kind.as_str() {
            "sql.view" => run_sql_view_transform(&mut adapter, &pack.root, transform, &params)
                .expect("sql transform runs"),
            "marker.extract_bracket_fields" => run_marker_extract_bracket_fields_transform(
                &mut adapter,
                transform,
                &params,
                &json!({}),
            )
            .expect("marker transform runs"),
            "rules.classify" => run_rules_classify_transform(&mut adapter, &pack, transform)
                .expect("rules transform runs"),
            other => panic!("unsupported transform kind in pack fixture: {other}"),
        }
    }

    let sql_transform = pack
        .transforms
        .iter()
        .find(|transform| transform.kind == "sql.view" && transform.sql.is_some())
        .expect("sql view transform");
    let output_table = &sql_transform.output.table;
    let rows = adapter
        .query_json(&format!("SELECT * FROM {output_table} ORDER BY start_ts"))
        .expect("query derived");
    assert_eq!(rows.len(), 3);

    let marker_transform = pack
        .transforms
        .iter()
        .find(|transform| transform.kind == "marker.extract_bracket_fields")
        .expect("marker transform");
    let marker_rows = adapter
        .query_json(&format!(
            "SELECT start_ts, end_ts FROM {} ORDER BY start_ts",
            marker_transform.output.table
        ))
        .expect("query marker output");
    assert_eq!(marker_rows.len(), 1);
    assert_eq!(marker_rows[0]["start_ts"], 100);
    assert_eq!(marker_rows[0]["end_ts"], 160);

    let rules_transform = pack
        .transforms
        .iter()
        .find(|transform| transform.kind == "rules.classify")
        .expect("rules transform");
    let classify_rows = adapter
        .query_json(&format!(
            "SELECT class FROM {} ORDER BY itid",
            rules_transform.output.table
        ))
        .expect("query classify output");
    assert_eq!(classify_rows.len(), 3);
    assert!(
        classify_rows
            .iter()
            .any(|row| row["class"].as_str() != Some("unclassified")),
        "expected pack rules to classify at least one fixture row"
    );

    let wakeup_rows = adapter
        .query_json("SELECT target_itid, waker_itid FROM wakeup_edges ORDER BY wake_ts")
        .expect("query wakeup edges");
    assert_eq!(wakeup_rows.len(), 1);
    assert_eq!(wakeup_rows[0]["target_itid"], 7);
    assert_eq!(wakeup_rows[0]["waker_itid"], 8);

    let run_store =
        AnalysisRunStore::create(dir.path().join("runs"), "run-fixture").expect("run store");
    run_store
        .write_plan(&json!({
            "runId": "run-fixture",
            "analysisId": analysis_id,
            "datasetRef": "sqlite:fixture"
        }))
        .expect("plan");
    run_store
        .write_state(&json!({
            "frontier": { "nextCandidateEdges": [] },
            "visitedEdges": [],
            "depth": 0,
            "coverage": { "explainedIntervals": [] }
        }))
        .expect("state");
    run_store
        .append_evidence(&json!({
            "evidenceId": format!("ev.{}.fixture", sql_transform.id),
            "tableRefs": [output_table],
            "facts": { "rows": rows.len() },
            "limitations": []
        }))
        .expect("evidence");
    run_store.render_checklist().expect("checklist");
    run_store
        .write_report("# Facts\n\n- fixture rows: 3\n\n# Inferences\n\n- none\n\n# Uncertainty\n\n- fixture data only\n")
        .expect("report");

    let run_dir = dir.path().join("runs/run-fixture");
    let plan_path = run_dir.join("plan.json");
    let state_path = run_dir.join("state.json");
    let evidence_path = run_dir.join("evidence.jsonl");
    let checklist_path = run_dir.join("checklist.md");
    let report_path = run_dir.join("report.md");

    assert!(plan_path.is_file());
    assert!(state_path.is_file());
    assert!(evidence_path.is_file());
    assert!(checklist_path.is_file());
    assert!(report_path.is_file());

    let plan: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan_path).expect("plan file")).expect("plan json");
    assert_eq!(plan["analysisId"], json!(analysis_id));

    let evidence_raw = std::fs::read_to_string(&evidence_path).expect("evidence file");
    let first_evidence_line = evidence_raw.lines().next().expect("evidence line");
    let evidence: serde_json::Value =
        serde_json::from_str(first_evidence_line).expect("evidence json");
    assert_eq!(evidence["facts"]["rows"], json!(3));
    assert!(
        evidence["tableRefs"]
            .as_array()
            .expect("table refs")
            .contains(&json!(output_table))
    );
}

#[test]
#[ignore = "requires local untracked test/test.db"]
fn pack_loads_against_local_test_db() {
    let raw_db = workspace_root().join("test/test.db");
    assert!(raw_db.is_file(), "missing {}", raw_db.display());
    let dir = tempdir().expect("tempdir");
    let scratch_db = dir.path().join("scratch.db");
    let pack = load_pack(fixture_pack_root()).expect("pack loads");
    let mut adapter = SQLiteDatasetAdapter::open(&raw_db, &scratch_db).expect("adapter");
    assert!(adapter.table_exists("thread_state").expect("table check"));
    assert!(
        pack.transforms
            .iter()
            .any(|transform| transform.kind == "sql.view" && transform.sql.is_some())
    );
    assert!(
        pack.transforms
            .iter()
            .any(|transform| transform.kind == "marker.extract_bracket_fields")
    );
    assert!(
        pack.transforms
            .iter()
            .any(|transform| transform.kind == "rules.classify")
    );
}

fn create_fixture_db(path: &Path, pack: &kat_rs_cli::trace_runtime::pack::LoadedPack) {
    let conn = Connection::open(path).expect("raw db");
    conn.execute(
        "CREATE TABLE thread_state (itid INTEGER, ts INTEGER, dur INTEGER, state TEXT)",
        [],
    )
    .expect("thread_state");
    conn.execute(
        "INSERT INTO thread_state VALUES (7, 10, 5, 'R'), (7, 20, 10, 'S'), (8, 30, 5, 'R'), (7, 100, 60, 'Running')",
        [],
    )
    .expect("thread_state rows");
    conn.execute(
        "CREATE TABLE process (ipid INTEGER, pid INTEGER, name TEXT)",
        [],
    )
    .expect("process");
    conn.execute(
        "INSERT INTO process VALUES (89, 15040, '.tencent.wechat'), (90, 42, 'render_service')",
        [],
    )
    .expect("process rows");
    conn.execute(
        "CREATE TABLE thread (itid INTEGER, tid INTEGER, ipid INTEGER, thread_name TEXT)",
        [],
    )
    .expect("thread");
    let matching_thread_name = first_rule_include(pack);
    conn.execute(
        "INSERT INTO thread VALUES (7, 15040, 89, ?1), (8, 42, 90, 'plain-worker'), (9, 43, 90, 'render-thread')",
        [matching_thread_name.as_str()],
    )
    .expect("thread rows");
    conn.execute(
        "CREATE TABLE callstack (id INTEGER, callid INTEGER, parent_id INTEGER, name TEXT, ts INTEGER, dur INTEGER)",
        [],
    )
    .expect("callstack");
    conn.execute(
        "INSERT INTO callstack VALUES
            (100, 7, NULL, 'H:UIVsyncTask[timestamp:90][vsyncID:1]|M0001', 90, 80),
            (101, 7, 100, 'H:UIVsyncTask[timestamp:90][vsyncID:1][layoutMeasureDurationStartTimestamp:100][layoutMeasureDurationEndTimestamp:160][firstDrawFrame:1]|M0001', 100, 60),
            (102, 7, 101, 'layout', 110, 20),
            (200, 9, NULL, 'RenderService::Draw', 165, 20)",
        [],
    )
    .expect("callstack rows");
    conn.execute(
        "CREATE TABLE frame_slice (id INTEGER, itid INTEGER, ipid INTEGER, ts INTEGER, dur INTEGER, src TEXT)",
        [],
    )
    .expect("frame_slice");
    conn.execute(
        "INSERT INTO frame_slice VALUES
            (501, 7, 89, 95, 70, ''),
            (601, 9, 90, 165, 20, 'from frame 501')",
        [],
    )
    .expect("frame_slice rows");
    for table in [
        "file_system_sample",
        "bio_latency_sample",
        "diskio",
        "syscall",
    ] {
        conn.execute(
            &format!("CREATE TABLE {table} (ts INTEGER, dur INTEGER, name TEXT)"),
            [],
        )
        .expect("io sample table");
        conn.execute(
            &format!("INSERT INTO {table} VALUES (120, 5, '{table}')"),
            [],
        )
        .expect("io sample row");
    }
    conn.execute(
        "CREATE TABLE instant (ts INTEGER, ref INTEGER, wakeup_from INTEGER, name TEXT)",
        [],
    )
    .expect("instant");
    conn.execute(
        "INSERT INTO instant VALUES
            (130, 7, 8, 'sched_wakeup'),
            (140, 8, 9, 'sched_wakeup')",
        [],
    )
    .expect("instant rows");
}

fn first_rule_include(pack: &kat_rs_cli::trace_runtime::pack::LoadedPack) -> String {
    pack.rule_sets
        .iter()
        .flat_map(|rule_set| rule_set.rules.values())
        .find_map(|rule| {
            rule.get("contains")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    rule.get("any")
                        .and_then(|value| value.as_array())
                        .and_then(|values| values.first())
                        .and_then(|value| value.as_str())
                })
        })
        .expect("pack classify include value")
        .to_string()
}

fn fixture_pack_root() -> PathBuf {
    let packs_dir = workspace_root().join("packs");
    let mut packs = std::fs::read_dir(&packs_dir)
        .expect("packs dir")
        .map(|entry| entry.expect("pack dir entry").path())
        .filter(|path| path.join("pack.yaml").is_file())
        .collect::<Vec<_>>();
    packs.sort();
    packs
        .into_iter()
        .find(|path| {
            load_pack(path)
                .map(|pack| {
                    pack.transforms
                        .iter()
                        .any(|transform| transform.kind == "sql.view" && transform.sql.is_some())
                })
                .unwrap_or(false)
        })
        .unwrap_or_else(|| {
            panic!(
                "expected compatible fixture pack in {}",
                packs_dir.display()
            )
        })
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
