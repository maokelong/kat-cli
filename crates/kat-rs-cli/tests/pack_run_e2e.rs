use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("kat-rs-cli is under crates/")
        .to_path_buf()
}

fn clean_python() -> PathBuf {
    env::var_os("KAT_RS_PYTHON")
        .map(PathBuf::from)
        .expect("KAT_RS_PYTHON must point to a clean-venv Python executable")
}

fn assert_success(label: &str, output: Output) -> Output {
    assert!(
        output.status.success(),
        "{label} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn run_cli(python: &Path, args: Vec<OsString>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kat-rs"));
    command
        .args(args)
        .env("KAT_RS_PYTHON", python)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    assert_success("kat-rs", command.output().expect("start kat-rs"))
}

fn create_synthetic_sqlite(path: &Path) {
    let connection = Connection::open(path).expect("create synthetic SQLite");
    connection
        .execute_batch(
            r#"
CREATE TABLE thread_state(
  id INT, ts INT, dur INT, cpu INT, itid INT, tid INT, pid INT,
  state TEXT, arg_setid INT
);
INSERT INTO thread_state VALUES
  (1, 0, 400000, 0, 1, 10, 1000, 'S', NULL),
  (2, 400000, 100000, 0, 1, 10, 1000, 'R', NULL),
  (3, 0, 400000, 1, 2, 20, 1000, 'Running', NULL);

CREATE TABLE thread(
  id INT, itid INT, tid INT, name TEXT, start_ts INT, end_ts INT,
  ipid INT, is_main_thread INT, switch_count INT
);
INSERT INTO thread VALUES
  (1, 1, 10, 'main', 0, 500000, 100, 1, 2),
  (2, 2, 20, 'worker', 0, 500000, 100, 0, 1);

CREATE TABLE process(
  id INT, ipid INT, pid INT, name TEXT, start_ts INT, switch_count INT,
  thread_count INT, slice_count INT, mem_count INT
);
INSERT INTO process VALUES
  (1, 100, 1000, '.tencent.wechat', 0, 3, 2, 3, 0);

CREATE TABLE args(key INT, datatype INT, value INT, argset INT);
CREATE TABLE data_dict(id INT, data TEXT);

CREATE TABLE instant(
  ts INT, name TEXT, ref INT, wakeup_from INT, ref_type TEXT, value REAL
);
INSERT INTO instant VALUES
  (400000, 'sched_wakeup', 1, 2, 'itid', NULL);

CREATE TABLE sched_slice(
  id INT, ts INT, dur INT, ts_end INT, cpu INT, itid INT, ipid INT,
  end_state TEXT, priority INT, arg_setid INT
);
INSERT INTO sched_slice VALUES
  (1, 0, 400000, 400000, 1, 2, 100, 'R', 120, NULL);

CREATE TABLE callstack(
  id INT, ts INT, dur INT, callid INT, cat TEXT, name TEXT, depth INT,
  cookie INT, parent_id INT, argsetid INT, chainId TEXT, spanId TEXT,
  parentSpanId TEXT, flag TEXT, trace_level TEXT, trace_tag TEXT,
  custom_category TEXT, custom_args TEXT, child_callid INT
);
INSERT INTO callstack VALUES
  (1, 0, 400000, 2, '', 'worker_stack', 0, 0, NULL, NULL,
   NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);

CREATE TABLE frame_slice(
  id INT, ts INT, vsync INT, ipid INT, itid INT, callstack_id INT,
  dur INT, src TEXT, dst INT, type INT, type_desc TEXT, flag INT,
  depth INT, frame_no INT
);
INSERT INTO frame_slice VALUES
  (1, 0, 0, 100, 1, NULL, 500000, '', 0, 0, '', 0, 0, 1);
"#,
        )
        .expect("populate synthetic SQLite");
}

fn run_end_to_end(db: &Path, profile: &str) {
    let root = repo_root();
    let python = clean_python();
    let temporary = tempdir().expect("E2E tempdir");
    let dataset = temporary.path().join("dataset");
    let run_dir = temporary.path().join("run");
    let pack = root.join("packs").join("openharmony-critical-path");

    run_cli(
        &python,
        vec![
            "dataset".into(),
            "materialize".into(),
            "sqlite".into(),
            db.as_os_str().to_owned(),
            dataset.as_os_str().to_owned(),
        ],
    );
    let inspect_dataset = run_cli(
        &python,
        vec![
            "dataset".into(),
            "inspect".into(),
            dataset.as_os_str().to_owned(),
        ],
    );
    assert!(String::from_utf8_lossy(&inspect_dataset.stdout).contains("thread_state"));

    let inspect_pack = run_cli(
        &python,
        vec![
            "pack".into(),
            "inspect".into(),
            pack.as_os_str().to_owned(),
            "--json".into(),
        ],
    );
    let discovery: Value =
        serde_json::from_slice(&inspect_pack.stdout).expect("parse Pack discovery JSON");
    assert!(
        discovery["workflows"]
            .as_array()
            .expect("workflow array")
            .iter()
            .any(|item| item["name"] == "wechat_first_frame_critical_path")
    );

    run_cli(
        &python,
        vec![
            "pack".into(),
            "run".into(),
            pack.as_os_str().to_owned(),
            "wechat_first_frame_critical_path".into(),
            "--dataset".into(),
            dataset.as_os_str().to_owned(),
            "--run-dir".into(),
            run_dir.as_os_str().to_owned(),
        ],
    );

    let verifier = root.join("python").join("tests").join("verify_cli_e2e.py");
    let output = Command::new(&python)
        .arg("-I")
        .arg(verifier)
        .arg("--profile")
        .arg(profile)
        .arg("--db")
        .arg(db)
        .arg("--dataset")
        .arg(&dataset)
        .arg("--run-dir")
        .arg(&run_dir)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .output()
        .expect("start Python E2E verifier");
    let output = assert_success("Python E2E verifier", output);
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
#[ignore = "requires a clean-wheel KAT_RS_PYTHON; Full CI runs this explicitly"]
fn synthetic_sqlite_pack_run_e2e() {
    let temporary = tempdir().expect("synthetic fixture tempdir");
    let db = temporary.path().join("synthetic.db");
    create_synthetic_sqlite(&db);
    run_end_to_end(&db, "synthetic");
}

#[test]
#[ignore = "requires hash-pinned test.db and a clean-wheel KAT_RS_PYTHON"]
fn real_test_db_pack_run_e2e() {
    let db = env::var_os("KAT_RS_E2E_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("test").join("test.db"));
    assert!(
        db.is_file(),
        "real E2E database is missing; set KAT_RS_E2E_DB or provide {}",
        db.display(),
    );
    assert_eq!(
        fs::metadata(&db).expect("read test.db metadata").len(),
        61_009_920,
        "unexpected test.db length",
    );
    run_end_to_end(&db, "real");
}
