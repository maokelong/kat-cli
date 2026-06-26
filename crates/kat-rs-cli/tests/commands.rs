use std::{ffi::OsString, path::PathBuf, time::Duration};

use clap::{CommandFactory, Parser};
use kat_rs_cli::commands::{Cli, run};
use tempfile::tempdir;

#[test]
fn top_level_help_lists_runtime_commands_without_query_or_daemon() {
    let help = Cli::command().render_long_help().to_string();

    assert!(help.contains("serve"));
    assert!(help.contains("stop"));
    assert!(help.contains("openapi"));
    assert!(help.contains("version"));
    assert!(!help.contains("query"));
    assert!(!help.contains("daemon"));
}

#[test]
fn removed_business_and_daemon_commands_are_rejected() {
    for command in ["query", "daemon"] {
        let error = Cli::try_parse_from(["kat-rs", command])
            .expect_err("removed command is rejected by clap");

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}

#[test]
fn analyze_command_parses_experimental_runtime_args() {
    let cli = Cli::try_parse_from([
        "kat-rs",
        "analyze",
        "--db",
        "test/test.db",
        "--pack",
        "packs/openharmony-core",
        "--analysis",
        "openharmony.critical_path",
        "--target-process",
        ".tencent.wechat",
        "--marker",
        "firstDrawFrame:1",
        "--run-id",
        "wechat-first-draw",
    ])
    .expect("analyze args parse");

    match cli.command {
        kat_rs_cli::commands::Command::Analyze(args) => {
            assert_eq!(args.db, PathBuf::from("test/test.db"));
            assert_eq!(args.pack, PathBuf::from("packs/openharmony-core"));
            assert_eq!(args.analysis, "openharmony.critical_path");
            assert_eq!(args.target_process, ".tencent.wechat");
            assert_eq!(args.marker, "firstDrawFrame:1");
            assert_eq!(args.run_id, "wechat-first-draw");
            assert_eq!(args.run_root, PathBuf::from(".kat/runs"));
            assert!(args.scratch_db.is_none());
        }
        other => panic!("expected analyze command, got {other:?}"),
    }
}

#[tokio::test]
async fn analyze_command_creates_scratch_parent_before_running_analysis() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("missing.db");
    let run_root = dir.path().join("missing-runs");
    let scratch_db = run_root.join("missing-db.scratch.db");
    let pack = openharmony_pack_path();
    let cli = Cli::try_parse_from([
        OsString::from("kat-rs"),
        OsString::from("analyze"),
        OsString::from("--db"),
        raw_db.clone().into_os_string(),
        OsString::from("--pack"),
        pack.into_os_string(),
        OsString::from("--analysis"),
        OsString::from("openharmony.critical_path"),
        OsString::from("--target-process"),
        OsString::from(".tencent.wechat"),
        OsString::from("--run-id"),
        OsString::from("missing-db"),
        OsString::from("--run-root"),
        run_root.clone().into_os_string(),
    ])
    .expect("analyze args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        scratch_db.parent().expect("scratch parent").exists(),
        "scratch parent should be created before analysis opens databases"
    );
    assert!(
        String::from_utf8_lossy(&err).contains("input table `callstack` does not exist"),
        "stderr: {}",
        String::from_utf8_lossy(&err)
    );
}

#[tokio::test]
async fn analyze_command_rejects_invalid_run_id_before_scratch_setup() {
    let dir = tempdir().expect("tempdir");
    let raw_db = dir.path().join("missing.db");
    let run_root = dir.path().join("runs");
    let escaped_scratch_db = dir.path().join("escape.scratch.db");
    let cli = Cli::try_parse_from([
        OsString::from("kat-rs"),
        OsString::from("analyze"),
        OsString::from("--db"),
        raw_db.into_os_string(),
        OsString::from("--pack"),
        openharmony_pack_path().into_os_string(),
        OsString::from("--analysis"),
        OsString::from("openharmony.critical_path"),
        OsString::from("--target-process"),
        OsString::from(".tencent.wechat"),
        OsString::from("--run-id"),
        OsString::from("..\\escape"),
        OsString::from("--run-root"),
        run_root.clone().into_os_string(),
    ])
    .expect("analyze args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        String::from_utf8_lossy(&err).contains("invalid analysis run id"),
        "stderr: {}",
        String::from_utf8_lossy(&err)
    );
    assert!(
        !run_root.exists(),
        "invalid run id should not create the run root"
    );
    assert!(
        !escaped_scratch_db.exists(),
        "invalid run id should not create scratch db outside run root"
    );
}

#[tokio::test]
async fn serve_rejects_non_loopback_host_before_serving() {
    let cli = Cli::try_parse_from(["kat-rs", "serve", "--host", "0.0.0.0", "--port", "3030"])
        .expect("serve args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = tokio::time::timeout(Duration::from_millis(100), run(cli, &mut out, &mut err))
        .await
        .expect("serve rejects before serving");

    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        String::from_utf8_lossy(&err).contains("loopback"),
        "stderr: {}",
        String::from_utf8_lossy(&err)
    );
}

#[tokio::test]
async fn stop_rejects_non_loopback_host_before_connecting() {
    let cli = Cli::try_parse_from(["kat-rs", "stop", "--host", "0.0.0.0", "--port", "3030"])
        .expect("stop args parse");
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

#[tokio::test]
async fn openapi_command_prints_openapi_json() {
    let cli = Cli::try_parse_from(["kat-rs", "openapi"]).expect("openapi args parse");
    let mut out = Vec::new();
    let mut err = Vec::new();

    let code = run(cli, &mut out, &mut err).await;

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert!(err.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out).expect("stdout json");
    assert_eq!(value, kat_rs_daemon::openapi_document());
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

fn openharmony_pack_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packs")
        .join("openharmony-core")
}
