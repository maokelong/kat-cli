use std::{fs, fs::File, io::Write, path::Path};

use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn registers_legacy_langfuse_jsonl_gz_tables_for_sql_queries() {
    let dir = tempdir().expect("tempdir is created");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");

    write_jsonl_gz(
        &observations_path,
        &[
            r#"{"id":"obs-1","trace_id":"trace-1","type":"GENERATION","name":"llm","start_time":"2026-06-15 01:00:00.000000","end_time":"2026-06-15 01:00:02.000000","input":"full prompt text without truncation","output":"full completion text without truncation"}"#,
            r#"{"id":"obs-2","trace_id":"trace-1","type":"SPAN","name":"tool","start_time":"2026-06-15 01:00:02.000000","end_time":"2026-06-15 01:00:03.000000","input":"tool input","output":"tool output"}"#,
        ],
    );
    write_jsonl_gz(
        &traces_path,
        &[
            r#"{"id":"trace-1","name":"chat request","user_id":"user-1","session_id":"session-1","input":"full trace input","output":"full trace output"}"#,
        ],
    );

    let datasource =
        kat_datasource::TraceDatasource::from_langfuse_legacy(&observations_path, &traces_path)
            .await
            .expect("datasource builds");

    let rows = datasource
        .query_json(
            "select o.id, o.input, o.output, t.name as trace_name \
             from langfuse_observations o \
             join langfuse_traces t on o.trace_id = t.id \
             where o.type = 'GENERATION'",
        )
        .await
        .expect("query succeeds");

    assert_eq!(
        rows,
        json!([{
            "id": "obs-1",
            "input": "full prompt text without truncation",
            "output": "full completion text without truncation",
            "trace_name": "chat request",
        }])
    );
}

#[tokio::test]
async fn build_materializes_legacy_langfuse_tables_without_source_files() {
    let dir = tempdir().expect("tempdir is created");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");

    write_jsonl_gz(
        &observations_path,
        &[
            r#"{"id":"obs-1","trace_id":"trace-1","type":"GENERATION","input":"prompt","output":"completion"}"#,
        ],
    );
    write_jsonl_gz(
        &traces_path,
        &[r#"{"id":"trace-1","name":"chat request","user_id":"user-1"}"#],
    );

    let datasource =
        kat_datasource::TraceDatasource::from_langfuse_legacy(&observations_path, &traces_path)
            .await
            .expect("datasource builds");

    fs::remove_file(&observations_path).expect("observations source can be removed");
    fs::remove_file(&traces_path).expect("traces source can be removed");

    let rows = datasource
        .query_json(
            "select o.id, t.name as trace_name \
             from langfuse_observations o \
             join langfuse_traces t on o.trace_id = t.id",
        )
        .await
        .expect("query succeeds after source files are gone");

    assert_eq!(
        rows,
        json!([{ "id": "obs-1", "trace_name": "chat request" }])
    );
}

#[tokio::test]
async fn rejects_invalid_langfuse_jsonl_gz_without_parse_error_table() {
    let dir = tempdir().expect("tempdir is created");
    let observations_path = dir.path().join("observations.jsonl.gz");
    let traces_path = dir.path().join("traces.jsonl.gz");

    fs::write(&observations_path, b"not a gzip stream").expect("bad fixture is written");
    write_jsonl_gz(&traces_path, &[r#"{"id":"trace-1","name":"chat request"}"#]);

    let result =
        kat_datasource::TraceDatasource::from_langfuse_legacy(&observations_path, &traces_path)
            .await;
    let Err(error) = result else {
        panic!("bad gzip is rejected");
    };
    let message = format!("{error:#}");

    assert!(
        message.contains("failed to register Langfuse JSONL table langfuse_observations"),
        "{message}"
    );
}

fn write_jsonl_gz(path: &Path, lines: &[&str]) {
    let file = File::create(path).expect("gzip fixture file is created");
    let mut encoder = GzEncoder::new(file, Compression::default());

    for line in lines {
        writeln!(encoder, "{line}").expect("jsonl line is written");
    }

    encoder.finish().expect("gzip stream is finished");
}
