use std::{fs, path::Path, process::Command};

use rusqlite::Connection;
use serde_json::Value;

#[allow(dead_code)]
mod support;

fn command(binary: &Path) -> Command {
    Command::new(binary)
}

#[test]
fn config_set_data_home_creates_it_and_get_returns_its_canonical_path() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    let data_home = temporary.path().join("analysis").join("state");

    let set = command(&binary)
        .args(["config", "set", "data-home", data_home.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(data_home.is_dir());

    let expected = dunce::canonicalize(&data_home).unwrap();
    let set: Value = serde_json::from_slice(&set.stdout).unwrap();
    assert_eq!(
        set.pointer("/result/data_home").and_then(Value::as_str),
        expected.to_str()
    );
    let stored: Value =
        serde_json::from_slice(&fs::read(skill.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        stored.get("data_home").and_then(Value::as_str),
        expected.to_str()
    );

    let get = command(&binary)
        .args(["config", "get", "data-home"])
        .output()
        .unwrap();
    assert!(
        get.status.success(),
        "{}",
        String::from_utf8_lossy(&get.stderr)
    );
    let get: Value = serde_json::from_slice(&get.stdout).unwrap();
    assert_eq!(
        get.pointer("/result/data_home").and_then(Value::as_str),
        expected.to_str()
    );
}

#[test]
fn import_without_a_dataset_uses_the_configured_data_home() {
    let temporary = tempfile::tempdir().unwrap();
    let (_, binary) = support::stage_skill(temporary.path(), "skill");
    let data_home = temporary.path().join("analysis").join("state");
    let source = temporary.path().join("source.db");
    Connection::open(&source)
        .unwrap()
        .execute_batch("CREATE TABLE thread (itid INTEGER, tid INTEGER, name TEXT);")
        .unwrap();

    let configured = command(&binary)
        .args(["config", "set", "data-home", data_home.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(configured.status.success());

    let imported = command(&binary)
        .args([
            "import",
            "trace-streamer",
            "--database",
            source.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let imported: Value = serde_json::from_slice(&imported.stdout).unwrap();
    let dataset = std::path::PathBuf::from(
        imported
            .pointer("/result/path")
            .and_then(Value::as_str)
            .unwrap(),
    );
    assert_eq!(
        dataset.parent(),
        Some(
            dunce::canonicalize(&data_home)
                .unwrap()
                .join("datasets")
                .as_path()
        )
    );
    assert!(dataset.join(".kat-dataset").is_file());
}

#[test]
fn config_set_recovers_from_an_invalid_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let (skill, binary) = support::stage_skill(temporary.path(), "skill");
    fs::write(skill.join("config.json"), "{\"other\":\"value\"}").unwrap();

    let get = command(&binary)
        .args(["config", "get", "data-home"])
        .output()
        .unwrap();
    assert!(!get.status.success());
    let get: Value = serde_json::from_slice(&get.stdout).unwrap();
    assert!(
        get.pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.starts_with("KAT Configuration is invalid:"))
    );

    let inspect = command(&binary)
        .args([
            "inspect",
            "--dataset",
            temporary.path().join("missing-dataset").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!inspect.status.success());
    let inspect: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert!(
        inspect
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.starts_with("KAT Configuration is invalid:"))
    );

    fs::write(skill.join("config.json"), "{\"data_home\":\".\"}").unwrap();
    let relative = command(&binary)
        .args(["config", "get", "data-home"])
        .output()
        .unwrap();
    assert!(!relative.status.success());
    let relative: Value = serde_json::from_slice(&relative.stdout).unwrap();
    assert!(
        relative
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.starts_with("KAT Configuration is invalid:"))
    );

    let data_home = temporary.path().join("recovered-data-home");
    let set = command(&binary)
        .args(["config", "set", "data-home", data_home.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let stored: Value =
        serde_json::from_slice(&fs::read(skill.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        stored.get("data_home").and_then(Value::as_str),
        dunce::canonicalize(data_home).unwrap().to_str()
    );
}
