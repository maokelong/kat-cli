use prost::Message;

#[allow(dead_code)]
mod proto {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));

    pub mod kat {
        pub mod hitrace {
            include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
        }
    }
}

#[test]
fn generated_proto_includes_sched_switch_format() {
    let value = proto::SchedSwitchFormat {
        prev_comm: "render".to_string(),
        prev_pid: 42,
        prev_prio: 120,
        prev_state: 1,
        next_comm: "main".to_string(),
        next_pid: 7,
        next_prio: 100,
    };

    let decoded =
        proto::SchedSwitchFormat::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.prev_comm, "render");
    assert_eq!(decoded.prev_pid, 42);
    assert_eq!(decoded.prev_prio, 120);
    assert_eq!(decoded.prev_state, 1);
    assert_eq!(decoded.next_comm, "main");
    assert_eq!(decoded.next_pid, 7);
    assert_eq!(decoded.next_prio, 100);
}

#[test]
fn generated_proto_includes_upstream_sched_messages() {
    let value = proto::SchedBlockedReasonFormat {
        pid: 42,
        caller: 0xfeed_beef,
        io_wait: 1,
    };

    let decoded =
        proto::SchedBlockedReasonFormat::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.pid, 42);
    assert_eq!(decoded.caller, 0xfeed_beef);
    assert_eq!(decoded.io_wait, 1);
}

#[test]
fn generated_ftrace_event_uses_direct_sched_fields() {
    let value = proto::kat::hitrace::FtraceEvent {
        timestamp: 10,
        tgid: 500,
        comm: "source".to_string(),
        sched_switch_format: Some(proto::SchedSwitchFormat {
            prev_comm: "render".to_string(),
            prev_pid: 42,
            prev_prio: 120,
            prev_state: 1,
            next_comm: "main".to_string(),
            next_pid: 7,
            next_prio: 100,
        }),
        sched_blocked_reason_format: Some(proto::SchedBlockedReasonFormat {
            pid: 42,
            caller: 0xfeed_beef,
            io_wait: 1,
        }),
        ..Default::default()
    };

    let decoded =
        proto::kat::hitrace::FtraceEvent::decode(value.encode_to_vec().as_slice()).expect("decode");

    assert_eq!(decoded.timestamp, 10);
    assert!(decoded.sched_switch_format.is_some());
    assert!(decoded.sched_blocked_reason_format.is_some());
}
