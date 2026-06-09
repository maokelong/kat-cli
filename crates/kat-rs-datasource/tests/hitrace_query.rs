use std::fs;

use kat_rs_datasource::proto::{HitraceEvent, HitraceTrace};
use prost::Message;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn build_releases_mmap_and_queries_hitrace_as_json() {
    let dir = tempdir().expect("tempdir is created");
    let trace_path = dir.path().join("sample.hitrace");
    fs::write(&trace_path, encoded_trace()).expect("trace is written");

    let datasource = kat_rs_datasource::TraceDatasource::build(
        kat_rs_datasource::DataSourceConfig::hitrace(&trace_path),
    )
    .expect("datasource builds");

    fs::remove_file(&trace_path).expect("trace file can be removed after build");

    let rows = datasource
        .query_json("select count(*) as count, max(cpu) as max_cpu from hitrace_event")
        .await
        .expect("query succeeds");

    assert_eq!(rows, json!([{ "count": 2, "max_cpu": 7 }]));
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
