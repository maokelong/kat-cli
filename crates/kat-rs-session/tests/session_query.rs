use std::fs;

use kat_rs_datasource::proto::{HitraceEvent, HitraceTrace};
use prost::Message;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn session_stores_datasource_and_queries_json() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");

    let mut session = kat_rs_session::Session::create();
    session
        .build_datasource(kat_rs_datasource::DataSourceConfig::hitrace(&trace_path))
        .expect("datasource builds");

    let rows = session
        .query_json("select tag from hitrace_event order by timestamp_ns")
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "tag": "sched" }, { "tag": "app" }]));
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
