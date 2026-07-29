use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rusqlite::Connection;
use serde_json::Value;

#[allow(dead_code)]
mod support;

fn command(binary: &Path) -> Command {
    Command::new(binary)
}

fn source_database(root: &Path) -> PathBuf {
    let source = root.join("source.db");
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE thread (itid INTEGER, tid INTEGER, name TEXT);")
        .unwrap();
    source
}

fn import(binary: &Path, source: &Path, environment_home: Option<&Path>) -> Value {
    let mut command = command(binary);
    command.args([
        "import",
        "trace-streamer",
        "--database",
        source.to_str().unwrap(),
    ]);
    match environment_home {
        Some(path) => {
            command.env("KAT_DATA_HOME", path);
        }
        None => {
            command.env_remove("KAT_DATA_HOME");
        }
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn dataset_path(response: &Value) -> PathBuf {
    PathBuf::from(
        response
            .pointer("/result/path")
            .and_then(Value::as_str)
            .unwrap(),
    )
}

#[test]
fn file_data_home_overrides_kat_data_home_environment_variable() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    let configured_home = temporary.path().join("configured-home");
    let environment_home = temporary.path().join("environment-home");
    fs::create_dir_all(&configured_home).unwrap();
    fs::create_dir_all(&environment_home).unwrap();
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": configured_home }).to_string(),
    )
    .unwrap();

    let response = import(
        &binary,
        &source_database(temporary.path()),
        Some(&environment_home),
    );
    assert_eq!(
        dataset_path(&response).parent(),
        Some(
            dunce::canonicalize(&configured_home)
                .unwrap()
                .join("datasets")
                .as_path()
        )
    );
}

#[test]
fn empty_or_missing_file_value_falls_back_to_kat_data_home_environment_variable() {
    for configuration in [
        serde_json::json!({ "kat_data_home": "", "future_setting": true }),
        serde_json::json!({ "future_setting": true }),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let (skill, binary) = support::stage_skill(temporary.path(), "skill");
        let environment_home = temporary.path().join("environment-home");
        fs::create_dir_all(&environment_home).unwrap();
        fs::write(skill.join("config.json"), configuration.to_string()).unwrap();

        let response = import(
            &binary,
            &source_database(temporary.path()),
            Some(&environment_home),
        );
        assert_eq!(
            dataset_path(&response).parent(),
            Some(
                dunce::canonicalize(&environment_home)
                    .unwrap()
                    .join("datasets")
                    .as_path()
            )
        );
    }
}

#[test]
fn missing_file_falls_back_to_kat_data_home_environment_variable() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    fs::remove_file(skill.join("config.json")).unwrap();
    let environment_home = temporary.path().join("environment-home");
    fs::create_dir_all(&environment_home).unwrap();

    let response = import(
        &binary,
        &source_database(temporary.path()),
        Some(&environment_home),
    );
    assert_eq!(
        dataset_path(&response).parent(),
        Some(
            dunce::canonicalize(&environment_home)
                .unwrap()
                .join("datasets")
                .as_path()
        )
    );
}

#[test]
fn invalid_selected_values_fail_instead_of_falling_back() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    let environment_home = temporary.path().join("environment-home");
    fs::create_dir_all(&environment_home).unwrap();
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": "relative" }).to_string(),
    )
    .unwrap();

    let source = source_database(temporary.path());
    let output = command(&binary)
        .args([
            "import",
            "trace-streamer",
            "--database",
            source.to_str().unwrap(),
        ])
        .env("KAT_DATA_HOME", &environment_home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("absolute path"))
    );
}

#[test]
fn non_string_file_value_fails_instead_of_falling_back() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    let environment_home = temporary.path().join("environment-home");
    fs::create_dir_all(&environment_home).unwrap();
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": null }).to_string(),
    )
    .unwrap();

    let source = source_database(temporary.path());
    let output = command(&binary)
        .args([
            "import",
            "trace-streamer",
            "--database",
            source.to_str().unwrap(),
        ])
        .env("KAT_DATA_HOME", &environment_home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.starts_with("KAT Configuration is invalid:"))
    );
}

#[test]
fn invalid_kat_data_home_environment_variable_fails_instead_of_falling_back() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": "" }).to_string(),
    )
    .unwrap();
    let source = source_database(temporary.path());

    let output = command(&binary)
        .args([
            "import",
            "trace-streamer",
            "--database",
            source.to_str().unwrap(),
        ])
        .env("KAT_DATA_HOME", "relative")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("KAT_DATA_HOME must be an absolute path"))
    );
}

#[test]
fn config_subcommand_is_not_available() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let output = command(&binary)
        .args(["config", "--help"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
