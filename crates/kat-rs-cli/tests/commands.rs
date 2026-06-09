use std::fs;

use clap::{CommandFactory, Parser};
use kat_rs_cli::commands::{Cli, run};
use kat_rs_datasource::proto::{HitraceEvent, HitraceTrace};
use prost::Message;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn query_command_prints_json_rows() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");

    let cli = Cli::parse_from(vec![
        "kat-rs".to_string(),
        "query".to_string(),
        "--source".to_string(),
        "hitrace".to_string(),
        "--file".to_string(),
        trace_path.to_string_lossy().to_string(),
        "--sql".to_string(),
        "select count(*) as count from hitrace_event".to_string(),
    ]);
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(String::from_utf8(out).expect("utf8"), "[{\"count\":2}]\n");
    assert!(err.is_empty());
}

#[tokio::test]
async fn query_command_rejects_missing_required_arguments() {
    let error = Cli::try_parse_from(["kat-rs", "query", "--source", "hitrace"])
        .expect_err("missing args are rejected by clap");

    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn help_command_prints_usage() {
    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("query")
        .expect("query subcommand exists")
        .render_long_help()
        .to_string();

    assert!(help.contains("--source"));
    assert!(help.contains("--file"));
    assert!(help.contains("--sql"));
}

#[test]
fn query_command_rejects_unknown_source() {
    let error = Cli::try_parse_from([
        "kat-rs",
        "query",
        "--source",
        "unknown",
        "--file",
        "sample.hitrace",
        "--sql",
        "select 1",
    ])
    .expect_err("unknown source is rejected by clap");

    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn clap_arguments_are_serde_serializable() {
    let cli = Cli::parse_from([
        "kat-rs",
        "query",
        "--source",
        "hitrace",
        "--file",
        "sample.hitrace",
        "--sql",
        "select 1",
    ]);

    assert_eq!(
        serde_json::to_value(cli).expect("cli serializes"),
        json!({
            "command": {
                "query": {
                    "source": "hitrace",
                    "file": "sample.hitrace",
                    "sql": "select 1"
                }
            }
        })
    );
}

fn encoded_trace() -> Vec<u8> {
    HitraceTrace {
        events: vec![
            HitraceEvent {
                timestamp_ns: 100,
                pid: 10,
                tid: 11,
                tag: "sched".to_string(),
                message: "wake up".to_string(),
                cpu: 3,
            },
            HitraceEvent {
                timestamp_ns: 200,
                pid: 20,
                tid: 21,
                tag: "app".to_string(),
                message: "start".to_string(),
                cpu: 7,
            },
        ],
    }
    .encode_to_vec()
}
