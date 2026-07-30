use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rusqlite::Connection;
use serde_json::Value;

#[allow(dead_code)]
mod support;
#[path = "support/test_home.rs"]
mod test_home;

fn command(binary: &Path, root: &Path) -> Command {
    let mut command = Command::new(binary);
    test_home::configure(&mut command, root);
    command
}

fn source_database(root: &Path) -> PathBuf {
    let source = root.join("source.db");
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE thread (itid INTEGER, tid INTEGER, name TEXT);")
        .unwrap();
    source
}

fn import(binary: &Path, root: &Path, source: &Path, environment_home: Option<&Path>) -> Value {
    let mut command = command(binary, root);
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

fn configuration_path(root: &Path) -> PathBuf {
    test_home::data_home(root).join("config.json")
}

fn write_configuration(root: &Path, configuration: Value) {
    let path = configuration_path(root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, configuration.to_string()).unwrap();
}

fn import_command(binary: &Path, root: &Path, source: &Path) -> Command {
    let mut command = command(binary, root);
    command.args([
        "import",
        "trace-streamer",
        "--database",
        source.to_str().unwrap(),
    ]);
    command
}

#[test]
fn kat_data_home_environment_variable_overrides_platform_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let configured_home = temporary.path().join("configured-home");
    let environment_home = temporary.path().join("environment-home");
    fs::create_dir_all(&configured_home).unwrap();
    fs::create_dir_all(&environment_home).unwrap();
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": configured_home }),
    );

    let response = import(
        &binary,
        temporary.path(),
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
fn platform_configuration_is_used_when_environment_variable_is_empty_or_missing() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let configured_home = temporary.path().join("configured-home");
    fs::create_dir_all(&configured_home).unwrap();
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": configured_home, "future_setting": true }),
    );

    let source = source_database(temporary.path());
    let response = import(&binary, temporary.path(), &source, None);
    assert_eq!(
        dataset_path(&response).parent(),
        Some(
            dunce::canonicalize(&configured_home)
                .unwrap()
                .join("datasets")
                .as_path()
        )
    );

    let output = import_command(&binary, temporary.path(), &source)
        .env("KAT_DATA_HOME", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
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
fn missing_platform_configuration_falls_back_without_creating_a_file() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let configuration = configuration_path(temporary.path());
    assert!(!configuration.exists());

    let response = import(
        &binary,
        temporary.path(),
        &source_database(temporary.path()),
        None,
    );
    assert_eq!(
        dataset_path(&response).parent(),
        Some(
            test_home::data_home(temporary.path())
                .join("datasets")
                .as_path()
        )
    );
    assert!(!configuration.exists());
}

#[test]
fn skill_root_configuration_is_not_read() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    let configured_home = temporary.path().join("configured-home");
    let stale_skill_home = temporary.path().join("stale-skill-home");
    fs::create_dir_all(&configured_home).unwrap();
    fs::create_dir_all(&stale_skill_home).unwrap();
    fs::write(
        skill.join("config.json"),
        serde_json::json!({ "kat_data_home": stale_skill_home }).to_string(),
    )
    .unwrap();
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": configured_home }),
    );

    let response = import(
        &binary,
        temporary.path(),
        &source_database(temporary.path()),
        None,
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
fn invalid_platform_configuration_fails_when_environment_variable_is_missing() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": "relative" }),
    );

    let output = import_command(
        &binary,
        temporary.path(),
        &source_database(temporary.path()),
    )
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
fn non_string_platform_configuration_fails_when_environment_variable_is_missing() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": null }),
    );

    let output = import_command(
        &binary,
        temporary.path(),
        &source_database(temporary.path()),
    )
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
fn invalid_kat_data_home_environment_variable_fails_before_platform_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let configured_home = temporary.path().join("configured-home");
    fs::create_dir_all(&configured_home).unwrap();
    write_configuration(
        temporary.path(),
        serde_json::json!({ "kat_data_home": configured_home }),
    );

    let output = import_command(
        &binary,
        temporary.path(),
        &source_database(temporary.path()),
    )
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
    let output = command(&binary, temporary.path())
        .args(["config", "--help"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
