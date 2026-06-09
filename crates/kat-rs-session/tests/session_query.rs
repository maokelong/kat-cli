use std::fs;

use serde_json::json;
use tempfile::tempdir;

const PROFILER_HEADER_SIZE: usize = 1024;
const PROFILER_HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;
const HIPROFILER_PROTOBUF_BIN: u32 = 0;

#[tokio::test]
async fn session_stores_datasource_and_queries_json() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, empty_hitrace()).expect("trace is written");

    let mut session = kat_rs_session::Session::create();
    session
        .build_datasource(kat_rs_datasource::DataSourceConfig::hitrace(&trace_path))
        .expect("datasource builds");

    let rows = session
        .query_json("select 1 as ok")
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "ok": 1 }]));
}

#[tokio::test]
async fn session_rejects_query_before_datasource_build() {
    let session = kat_rs_session::Session::create();

    let error = session
        .query_json("select 1")
        .await
        .expect_err("query without datasource fails");

    assert!(error.to_string().contains("datasource is not built"));
}

fn empty_hitrace() -> Vec<u8> {
    let mut bytes = vec![0; PROFILER_HEADER_SIZE];
    bytes[0..8].copy_from_slice(&PROFILER_HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&(PROFILER_HEADER_SIZE as u64).to_le_bytes());
    bytes[56..60].copy_from_slice(&HIPROFILER_PROTOBUF_BIN.to_le_bytes());
    bytes
}
