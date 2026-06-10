use prost::Message;

#[allow(dead_code)]
mod proto {
    include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
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
