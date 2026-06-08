use prost::Message;
use trace_htrace::{parse_bytes, table_specs};

#[test]
fn generated_specs_include_htrace_tables() {
    let names = table_specs()
        .iter()
        .map(|table| table.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["process_event", "counter_event"]);
}

#[test]
fn parse_bytes_returns_dataset_with_non_empty_tables() {
    let trace = htrace_proto::kat::htrace::HtraceTrace {
        process_events: vec![htrace_proto::kat::htrace::ProcessEvent {
            timestamp_ns: 10,
            pid: 42,
            process_name: "wechat".to_string(),
        }],
        counter_events: vec![htrace_proto::kat::htrace::CounterEvent {
            timestamp_ns: 11,
            name: "rss".to_string(),
            value: 4096,
        }],
    };

    let dataset = parse_bytes(&trace.encode_to_vec()).expect("trace parses");
    let tables = dataset
        .tables()
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(tables, vec!["process_event", "counter_event"]);
}
