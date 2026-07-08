use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::json;
use tempfile::{TempDir, tempdir};

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
            worker_script: workspace_root()
                .join("python")
                .join("kat_worker")
                .join("kat_worker.py"),
            sdk_path: workspace_root().join("python").join("kat_sdk"),
            max_preview_rows: 50,
            max_bounded_rows: 1000,
        },
    );

    let summary = runner
        .run(kat_rs_daemon::pack_runtime::PackRunRequest {
            pack_root: fixture.pack_root.clone(),
            workflow_name: "workflows.extract".to_string(),
            inputs: json!({ "min_id": 2 }).as_object().unwrap().clone(),
            run_dir: fixture.dir.path().join("run"),
        })
        .await
        .expect("pack run succeeds");

    assert_eq!(
        summary.status,
        kat_rs_daemon::pack_runtime::PackRunStatus::Succeeded,
        "{:?}",
        summary.traceback
    );
    let artifact = summary
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "selected_threads")
        .expect("artifact exists");
    assert_eq!(artifact.row_count, 1);
    assert_eq!(artifact.preview, json!([{ "id": 2, "name": "worker" }]));
}

#[test]
fn sql_params_render_scalars_and_skip_string_literals() {
    let sql = "select ':not_param' as literal, :id as id, :name as name";
    let rendered = kat_rs_daemon::pack_runtime::render_sql_params(
        sql,
        &json!({ "id": 7, "name": "O'Hara" })
            .as_object()
            .unwrap()
            .clone(),
    )
    .expect("params render");

    assert_eq!(
        rendered,
        "select ':not_param' as literal, 7 as id, 'O''Hara' as name"
    );
}

#[test]
fn sql_params_reject_objects_and_arrays() {
    let error = kat_rs_daemon::pack_runtime::render_sql_params(
        "select :value",
        &json!({ "value": [1, 2] }).as_object().unwrap().clone(),
    )
    .expect_err("array param is rejected");

    assert!(
        format!("{error:#}").contains("only scalar SQL params are supported"),
        "{error:#}"
    );
}

#[tokio::test]
#[ignore = "requires local test/test.db fixture"]
async fn critical_path_pack_extracts_wechat_first_frame_window_from_test_db() {
    let db_path = workspace_root().join("test").join("test.db");
    assert!(
        db_path.exists(),
        "missing local fixture: {}",
        db_path.display()
    );

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
            worker_script: workspace_root()
                .join("python")
                .join("kat_worker")
                .join("kat_worker.py"),
            sdk_path: workspace_root().join("python").join("kat_sdk"),
            max_preview_rows: 50,
            max_bounded_rows: 100_000,
        },
    );

    let summary = runner
        .run(kat_rs_daemon::pack_runtime::PackRunRequest {
            pack_root: workspace_root().join("packs").join("critical-path"),
            workflow_name: "workflows.extract".to_string(),
            inputs: json!({
                "root_itid": 405,
                "start_ts": 245615162000i64,
                "end_ts": 246306873000i64,
                "max_depth": 8,
                "min_segment_ms": 0.1,
                "max_fact_rows": 100000
            })
            .as_object()
            .unwrap()
            .clone(),
            run_dir: dir.path().join("wechat-run"),
        })
        .await
        .expect("critical path pack run succeeds");

    assert_eq!(
        summary.status,
        kat_rs_daemon::pack_runtime::PackRunStatus::Succeeded,
        "{:?}",
        summary.traceback
    );
    assert_artifact_exists(&summary, "target_window");
    assert_artifact_exists(&summary, "path_nodes");
    assert_artifact_exists(&summary, "path_edges");
    assert_artifact_exists(&summary, "critical_path_evidence");
    assert_artifact_preview_contains(&summary, "target_window", "root_itid", json!(405));
    assert_artifact_preview_contains(&summary, "path_nodes", "itid", json!(405));
    assert_artifact_numeric_field_gt(&summary, "critical_path_evidence", "node_count", 0);
}

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
        fs::create_dir_all(pack_root.join("lib")).expect("pack lib dir");
        fs::write(pack_root.join("lib").join("__init__.py"), "").expect("pack lib init");
        fs::write(
            pack_root.join("lib").join("model.py"),
            r#"
SELECT_THREADS_SQL = "select id, name from thread where id >= :min_id order by id"
"#,
        )
        .expect("pack lib model is written");
        fs::write(
            pack_root.join("lib").join("artifacts.py"),
            r#"
from .model import SELECT_THREADS_SQL
"#,
        )
        .expect("pack lib artifacts is written");
        fs::write(
            pack_root.join("workflows").join("extract.py"),
            r#"
from kat import option, workflow
from lib.artifacts import SELECT_THREADS_SQL


@workflow(title="Select threads")
@option("--min-id", help="Minimum id", default=0)
def extract(min_id: int = 0):
    import kat

    try:
        kat.query("select missing_column from thread").rows(max_rows=1)
    except Exception:
        pass
    else:
        raise AssertionError("rows errors should be catchable by workflow code")

    rows = kat.query(
        SELECT_THREADS_SQL,
        min_id=min_id,
    )
    observed = rows.rows(max_rows=10)
    kat.log("selected rows", count=len(observed))
    return {"selected_threads": rows}
"#,
        )
        .expect("pack workflow is written");

        Self {
            dir,
            dataset_path,
            pack_root,
        }
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

fn assert_artifact_exists(summary: &kat_rs_daemon::pack_runtime::PackRunSummary, name: &str) {
    assert!(
        summary
            .artifacts
            .iter()
            .any(|artifact| artifact.name == name),
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
    assert!(
        value > min_value,
        "{artifact_name}.{key}={value} <= {min_value}"
    );
}
