use std::sync::Arc;

use arrow_array::{RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use kat_rs_datasource::{DataFusionQuery, QueryRequest, TraceDatasource};
use prost::Message;
use tempfile::NamedTempFile;
use trace_arrow::{ArrowTable, TraceDataset};
use trace_proto::kat::htrace::{HtraceTrace, ProcessEvent};

#[tokio::test]
async fn datafusion_query_counts_registered_dataset_table() {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "timestamp_ns",
        DataType::UInt64,
        false,
    )]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![Arc::new(UInt64Array::from(vec![100_u64]))],
    )
    .expect("batch is valid");
    let table = ArrowTable::new("process_event", schema, vec![batch]).expect("table is valid");
    let dataset = TraceDataset::from_tables([table]).expect("dataset is valid");

    let query = DataFusionQuery::new(dataset).expect("query is created");
    let response = query
        .query("select count(*) as total from process_event")
        .await
        .expect("sql succeeds");

    assert_eq!(response.row_count, 1);
    assert_eq!(response.rows[0].cells[0].name, "total");
    assert_eq!(response.rows[0].cells[0].value, "1");
}

#[tokio::test]
async fn datasource_query_parses_htrace_file_and_runs_sql() {
    let trace = HtraceTrace {
        process_events: vec![ProcessEvent {
            timestamp_ns: 100,
            pid: 42,
            process_name: "wechat".to_string(),
        }],
        counter_events: Vec::new(),
    };
    let file = NamedTempFile::new().expect("temp file is created");
    std::fs::write(file.path(), trace.encode_to_vec()).expect("trace file is written");

    let datasource = TraceDatasource::new();
    let response = datasource
        .query(QueryRequest::new(
            file.path(),
            "select pid, process_name from process_event",
        ))
        .await
        .expect("query succeeds");

    assert_eq!(response.row_count, 1);
    assert_eq!(response.rows[0].cells[0].value, "42");
    assert_eq!(response.rows[0].cells[1].value, "wechat");
}
