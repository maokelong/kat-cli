use std::{fs, fs::File, io::Write, path::Path, time::Duration};

use clap::{CommandFactory, Parser};
use flate2::{Compression, write::GzEncoder};
use kat_rs_cli::commands::{Cli, run};
use serde_json::json;
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[tokio::test]
async fn query_command_prints_json_rows() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, empty_hitrace()).expect("trace is written");

    let cli = Cli::parse_from(vec![
        "kat-rs".to_string(),
        "query".to_string(),
        "--source".to_string(),
        "hitrace".to_string(),
        "--file".to_string(),
        trace_path.to_string_lossy().to_string(),
        "--sql".to_string(),
        "select 1 as ok".to_string(),
    ]);
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(String::from_utf8(out).expect("utf8"), "[{\"ok\":1}]\n");
    assert!(err.is_empty());
}

#[tokio::test]
async fn query_command_prints_langfuse_json_rows() {
    let dir = tempdir().expect("tempdir is created");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");
    write_jsonl_gz(
        &observations_path,
        &[
            r#"{"id":"obs-1","trace_id":"trace-1","type":"GENERATION","input":"full prompt","output":"full completion"}"#,
        ],
    );
    write_jsonl_gz(
        &traces_path,
        &[r#"{"id":"trace-1","name":"chat request","user_id":"user-1"}"#],
    );

    let cli = Cli::try_parse_from(vec![
        "kat-rs".to_string(),
        "query".to_string(),
        "--source".to_string(),
        "langfuse".to_string(),
        "--observations-file".to_string(),
        observations_path.to_string_lossy().to_string(),
        "--traces-file".to_string(),
        traces_path.to_string_lossy().to_string(),
        "--sql".to_string(),
        "select o.id, t.name as trace_name from langfuse_observations o join langfuse_traces t on o.trace_id = t.id".to_string(),
    ])
    .expect("langfuse args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out).expect("stdout json"),
        json!([{ "id": "obs-1", "trace_name": "chat request" }])
    );
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

#[tokio::test]
async fn query_command_rejects_missing_langfuse_files() {
    let error = Cli::try_parse_from([
        "kat-rs", "query", "--source", "langfuse", "--sql", "select 1",
    ])
    .expect_err("missing langfuse files are rejected by clap");

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
    assert!(help.contains("--observations-file"));
    assert!(help.contains("--traces-file"));
    assert!(help.contains("--sql"));
}

#[test]
fn daemon_help_lists_start_and_stop() {
    let mut command = Cli::command();
    let help = command
        .find_subcommand_mut("daemon")
        .expect("daemon subcommand exists")
        .render_long_help()
        .to_string();

    assert!(help.contains("start"));
    assert!(help.contains("stop"));
}

#[tokio::test]
async fn daemon_start_rejects_non_loopback_host_before_serving() {
    let cli = Cli::try_parse_from([
        "kat-rs", "daemon", "start", "--host", "0.0.0.0", "--port", "3030",
    ])
    .expect("daemon start args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = tokio::time::timeout(Duration::from_millis(100), run(cli, &mut out, &mut err))
        .await
        .expect("daemon start rejects before serving");

    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        String::from_utf8_lossy(&err).contains("loopback"),
        "stderr: {}",
        String::from_utf8_lossy(&err)
    );
}

#[tokio::test]
async fn daemon_stop_rejects_non_loopback_host_before_connecting() {
    let cli = Cli::try_parse_from([
        "kat-rs", "daemon", "stop", "--host", "0.0.0.0", "--port", "3030",
    ])
    .expect("daemon stop args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        String::from_utf8_lossy(&err).contains("loopback"),
        "stderr: {}",
        String::from_utf8_lossy(&err)
    );
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

fn write_jsonl_gz(path: &Path, lines: &[&str]) {
    let file = File::create(path).expect("gzip fixture file is created");
    let mut encoder = GzEncoder::new(file, Compression::default());

    for line in lines {
        writeln!(encoder, "{line}").expect("jsonl line is written");
    }

    encoder.finish().expect("gzip stream is finished");
}

fn empty_hitrace() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&(PROFILER_HEADER_SIZE as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
}
