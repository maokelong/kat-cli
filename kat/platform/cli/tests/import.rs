use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rusqlite::Connection;

fn cargo_kat() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kat"))
}

fn stage_skill(root: &Path) -> PathBuf {
    let skill = root.join("skill");
    let target = if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let binary_name = if cfg!(windows) { "kat.exe" } else { "kat" };
    let payload = skill.join("scripts").join("targets").join(target);
    fs::create_dir_all(&payload).unwrap();
    fs::write(skill.join("SKILL.md"), "# KAT\n").unwrap();
    let binary = payload.join(binary_name);
    fs::copy(cargo_kat(), &binary).unwrap();
    binary
}

fn command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("HOME", root.join("home"))
        .env("APPDATA", root.join("app-data"))
        .env("LOCALAPPDATA", root.join("local-app-data"))
        .env("USERPROFILE", root.join("profile"));
    command
}

fn data_home(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("app-data").join("KAT").join("data")
    } else {
        root.join("xdg-data").join("kat")
    }
}

fn database(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE z_table (ratio REAL); INSERT INTO z_table VALUES (2.5); \
             CREATE TABLE a_table (id INTEGER, label TEXT); INSERT INTO a_table VALUES (7, 'render'); \
             CREATE VIEW a_view (id, label) AS SELECT id, label FROM a_table;",
        )
        .unwrap();
}

#[test]
fn trace_streamer_import_then_inspect_is_a_real_json_process_loop() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let cwd = temp.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let source = cwd.join("source.db");
    database(&source);
    let dataset = cwd.join("数据集");

    let imported = command(&binary, temp.path())
        .current_dir(&cwd)
        .args([
            "import",
            "trace-streamer",
            "--database",
            "source.db",
            "--dataset",
            "数据集",
        ])
        .output()
        .unwrap();

    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(imported.stderr.is_empty());
    let response: serde_json::Value = serde_json::from_slice(&imported.stdout).unwrap();
    assert_eq!(response["status"], "success");
    assert_eq!(
        response["result"],
        serde_json::json!({"path": dunce::canonicalize(&dataset).unwrap().to_str().unwrap()})
    );
    assert!(response.get("log_path").is_none());

    let inspected = command(&binary, temp.path())
        .current_dir(&cwd)
        .args(["inspect", "--dataset", "数据集"])
        .output()
        .unwrap();
    assert_eq!(
        inspected.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let inspection: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(
        inspection["result"]["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|table| table["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["a_table", "a_view", "z_table"]
    );
}

#[test]
fn default_target_is_uuid_v7_under_data_home_and_is_inspectable() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("source.db");
    database(&source);

    let output = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = PathBuf::from(response["result"]["path"].as_str().unwrap());
    assert_eq!(
        path.parent().unwrap(),
        data_home(temp.path()).join("datasets")
    );
    let id = uuid::Uuid::parse_str(path.file_name().unwrap().to_str().unwrap()).unwrap();
    assert_eq!(id.get_version_num(), 7);
    assert!(path.join(".kat-dataset").is_file());
}

#[test]
fn overwrite_requires_explicit_target_and_replaces_every_entry() {
    let temp = tempfile::tempdir().unwrap();
    let binary = stage_skill(temp.path());
    let source = temp.path().join("source.db");
    database(&source);
    let target = temp.path().join("dataset");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), "old").unwrap();

    let refused = command(&binary, temp.path())
        .args(["import", "--dataset"])
        .arg(&target)
        .args(["trace-streamer", "--database"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(target.join("keep").exists());
    let failure: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(failure["status"], "failure");

    let replaced = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .args(["--dataset"])
        .arg(&target)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();
    assert_eq!(
        replaced.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    assert!(!target.join("keep").exists());

    let parse_failure = command(&binary, temp.path())
        .args(["import", "trace-streamer", "--database"])
        .arg(&source)
        .arg("--overwrite-dataset")
        .output()
        .unwrap();
    assert_eq!(parse_failure.status.code(), Some(2));
    assert!(parse_failure.stdout.is_empty());
}

#[test]
fn help_marks_trace_streamer_deprecated_and_explains_overwrite_risk() {
    for arguments in [
        &["import", "--help"][..],
        &["import", "trace-streamer", "--help"][..],
    ] {
        let help = Command::new(cargo_kat()).args(arguments).output().unwrap();
        assert_eq!(help.status.code(), Some(0));
        let help = String::from_utf8(help.stdout).unwrap();
        for text in [
            "Deprecated",
            "table interface is unstable",
            "removed before the first formal release",
        ] {
            assert!(help.contains(text), "missing {text:?}: {help}");
        }
        if arguments.len() > 2 {
            for text in [
                "--database",
                "--overwrite-dataset",
                "Permanently deletes all existing contents",
                "unrecognized files",
                "Linked or mounted paths",
                "No backup, rollback, or failure recovery",
            ] {
                assert!(help.contains(text), "missing {text:?}: {help}");
            }
        }
    }
}
