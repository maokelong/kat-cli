use std::fs;

use clap::{CommandFactory, Parser};
use kat_rs_cli::commands::{Cli, run};
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

fn empty_hitrace() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&(PROFILER_HEADER_SIZE as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
}
