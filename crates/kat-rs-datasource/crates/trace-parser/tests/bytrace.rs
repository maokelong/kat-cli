use trace_parser::{detect_trace_format, parse_trace_bytes, TraceFormat};

const BYTRACE_TEXT: &[u8] = br#"# tracer: nop
#           TASK-PID       TGID    CPU#  ||||   TIMESTAMP  FUNCTION
#              | |           |       |   ||||      |         |
      waker-20      (     20) [001] ....   100.000000: sched_wakeup: comm=worker pid=10 prio=120 target_cpu=000
     worker-10      (     10) [000] ....   100.010000: sched_switch: prev_comm=swapper/0 prev_pid=0 prev_prio=120 prev_state=R ==> next_comm=worker next_pid=10 next_prio=120
     worker-10      (     10) [000] ....   100.030000: sched_switch: prev_comm=worker prev_pid=10 prev_prio=120 prev_state=S ==> next_comm=swapper/0 next_pid=0 next_prio=120
"#;

#[test]
fn detects_bytrace_text_input() {
    assert_eq!(detect_trace_format(BYTRACE_TEXT), TraceFormat::BytraceText);
}

#[test]
fn parses_bytrace_text_sched_events() {
    let parsed = parse_trace_bytes(BYTRACE_TEXT).expect("bytrace text should parse");

    assert!(parsed.trace_id.starts_with("bytrace:"));
    assert_eq!(parsed.clock_domain, "boottime");
    assert_eq!(parsed.start_ts, Some(100_000_000_000));
    assert_eq!(parsed.end_ts, Some(100_030_000_000));
    assert_eq!(parsed.tables.sched_slice.num_rows(), 2);
    assert!(parsed.tables.thread_state.num_rows() >= 2);
    assert!(parsed.tables.thread.num_rows() >= 2);
    assert_eq!(parsed.tables.trace_metadata.num_rows(), 3);
}

#[test]
fn exposes_only_bytrace_mapped_tables() {
    let parsed = parse_trace_bytes(BYTRACE_TEXT).expect("bytrace text should parse");
    let batches = parsed.batches();

    assert!(batches.contains_key("sched_slice"));
    assert!(batches.contains_key("thread_state"));
    assert!(batches.contains_key("raw_event"));
    assert!(!batches.contains_key("diskio"));
    assert!(!batches.contains_key("dma_fence"));
    assert!(!batches.contains_key("js_heap_nodes"));
}
