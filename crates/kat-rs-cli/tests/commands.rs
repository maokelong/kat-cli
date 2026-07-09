use std::{fs, path::Path};

use clap::{CommandFactory, Parser};
use kat_rs_cli::commands::{Cli, run};
use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn top_level_help_lists_short_lived_commands_without_daemon_surface() {
    let help = Cli::command().render_long_help().to_string();

    assert!(help.contains("dataset"));
    assert!(help.contains("pack"));
    assert!(help.contains("version"));
    assert!(!help.contains("serve"));
    assert!(!help.contains("stop"));
    assert!(!help.contains("openapi"));
}

#[test]
fn removed_daemon_commands_are_rejected() {
    for command in ["serve", "stop", "openapi", "daemon"] {
        let error = Cli::try_parse_from(["kat-rs", command])
            .expect_err("removed command is rejected by clap");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}

#[tokio::test]
async fn dataset_materialize_inspect_and_query_sqlite() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("trace.db");
    create_cli_sqlite_fixture(&db_path);
    let dataset_path = dir.path().join("dataset");

    let cli = Cli::try_parse_from([
        "kat-rs",
        "dataset",
        "materialize",
        "sqlite",
        db_path.to_str().expect("utf8 db path"),
        dataset_path.to_str().expect("utf8 dataset path"),
    ])
    .expect("materialize args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(cli, &mut out, &mut err).await;
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert!(dataset_path.join("catalog.json").exists());

    let inspect = Cli::try_parse_from([
        "kat-rs",
        "dataset",
        "inspect",
        dataset_path.to_str().expect("utf8 dataset path"),
    ])
    .expect("inspect args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(inspect, &mut out, &mut err).await;
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let stdout = String::from_utf8(out).expect("utf8 stdout");
    assert!(stdout.contains("thread_state"), "stdout: {stdout}");

    let query = Cli::try_parse_from([
        "kat-rs",
        "dataset",
        "query",
        dataset_path.to_str().expect("utf8 dataset path"),
        "--sql",
        "select count(*) as count from thread_state",
    ])
    .expect("query args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(query, &mut out, &mut err).await;
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let rows: Value = serde_json::from_slice(&out).expect("query stdout json");
    assert_eq!(rows, serde_json::json!([{ "count": 1 }]));
}

#[tokio::test]
async fn version_command_prints_package_version() {
    let cli = Cli::try_parse_from(["kat-rs", "version"]).expect("version args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(err.is_empty());
}

fn create_cli_sqlite_fixture(path: &Path) {
    let connection = Connection::open(path).expect("sqlite opens");
    connection
        .execute(
            "create table thread_state(
                id integer,
                ts integer,
                dur integer,
                itid integer,
                tid integer,
                pid integer,
                state text
            )",
            [],
        )
        .expect("table created");
    connection
        .execute(
            "insert into thread_state values (1, 1000, 200, 405, 15040, 15040, 'Running')",
            [],
        )
        .expect("row inserted");

    let _ = fs::metadata(path).expect("fixture db exists");
}
