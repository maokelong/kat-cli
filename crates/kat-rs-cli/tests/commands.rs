use std::time::Duration;

use clap::{CommandFactory, Parser};
use kat_rs_cli::commands::{Cli, run};

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
    assert_eq!(value["openapi"], "3.1.0");
    assert!(value["paths"]["/v1/datasources"].is_object());
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
