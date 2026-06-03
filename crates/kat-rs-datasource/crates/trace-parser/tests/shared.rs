use trace_parser::plugins::shared::{parse_trace_marker, TraceMarker};

#[test]
fn parses_trace_marker_begin() {
    assert_eq!(
        parse_trace_marker("B|42|render##phase=prepare,count=2"),
        Some(TraceMarker::Begin {
            callid: 42,
            name: "render##phase=prepare,count=2".to_string()
        })
    );
}
