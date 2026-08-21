use std::{fs, fs::File, path::Path};

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use prost::Message;

use crate::{
    DatasetWriteTarget, TextFtraceClock, import_hitrace, import_text_ftrace, inspect_dataset,
    proto::kat::hitrace::{
        self as p, FtraceCpuDetailMsg, FtraceCpuStatsMsg, FtraceEvent, PerCpuStatsMsg,
        TracePluginResult, ftrace_event,
    },
};

const HEADER_SIZE: usize = 1024;
const HEADER_MAGIC: u64 = 0x464F_5250_534F_484F;

#[derive(Clone, PartialEq, Message)]
struct Envelope {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "3")]
    data: Vec<u8>,
}

fn event(timestamp: u64, payload: ftrace_event::Event) -> FtraceEvent {
    FtraceEvent {
        timestamp,
        tgid: Some(70),
        comm: "worker".to_owned(),
        common_fields: Some(ftrace_event::CommonFileds {
            r#type: None,
            flags: None,
            preempt_count: None,
            pid: 7,
        }),
        event: Some(payload),
    }
}

fn same_source_events() -> Vec<FtraceEvent> {
    use ftrace_event::Event;
    let device = |major: u32, minor: u32| (u64::from(major) << 20) | u64::from(minor);
    vec![
        event(
            1_000_000_001,
            Event::BinderTransactionFormat(p::BinderTransactionFormat {
                debug_id: 101,
                target_node: 0,
                to_proc: 8,
                to_thread: 9,
                reply: 1,
                code: 0x12,
                flags: 0x10,
            }),
        ),
        event(
            1_000_000_002,
            Event::BinderTransactionReceivedFormat(p::BinderTransactionReceivedFormat {
                debug_id: 101,
            }),
        ),
        event(
            1_000_000_003,
            Event::BlockBioRemapFormat(p::BlockBioRemapFormat {
                dev: device(179, 0),
                sector: 200,
                nr_sector: 8,
                old_dev: device(179, 15),
                old_sector: 100,
                rwbs: "WS".into(),
            }),
        ),
        event(
            1_000_000_004,
            Event::BlockRqCompleteFormat(p::BlockRqCompleteFormat {
                dev: device(179, 0),
                sector: 200,
                nr_sector: 8,
                error: 0,
                rwbs: "WS".into(),
                cmd: "".into(),
            }),
        ),
        event(
            1_000_000_005,
            Event::BlockRqInsertFormat(p::BlockRqInsertFormat {
                dev: device(179, 0),
                sector: 200,
                nr_sector: 8,
                bytes: 4096,
                rwbs: "WS".into(),
                comm: "worker".into(),
                cmd: "".into(),
            }),
        ),
        event(
            1_000_000_006,
            Event::BlockRqIssueFormat(p::BlockRqIssueFormat {
                dev: device(179, 0),
                sector: 200,
                nr_sector: 8,
                bytes: 4096,
                rwbs: "WS".into(),
                comm: "worker".into(),
                cmd: "".into(),
            }),
        ),
        event(
            1_000_000_007,
            Event::CpuIdleFormat(p::CpuIdleFormat {
                state: u32::MAX,
                cpu_id: 0,
            }),
        ),
        event(
            1_000_000_008,
            Event::IpiEntryFormat(p::IpiEntryFormat {
                reason: "Function call interrupts".into(),
            }),
        ),
        event(
            1_000_000_009,
            Event::IpiExitFormat(p::IpiExitFormat {
                reason: "Function call interrupts".into(),
            }),
        ),
        event(
            1_000_000_010,
            Event::IpiRaiseFormat(p::IpiRaiseFormat {
                target_cpus: None,
                reason: "Function call interrupts".into(),
                target_mask: Some("00000000,00000001".into()),
            }),
        ),
        event(
            1_000_000_011,
            Event::IpiSendCpuFormat(p::IpiSendCpuFormat {
                target_cpu: 0,
                callsite: "callsite+0x1/0x2".into(),
                callback: "callback+0x0/0x1".into(),
            }),
        ),
        event(
            1_000_000_012,
            Event::IrqHandlerEntryFormat(p::IrqHandlerEntryFormat {
                irq: 13,
                name: "arch_timer".into(),
            }),
        ),
        event(
            1_000_000_013,
            Event::IrqHandlerExitFormat(p::IrqHandlerExitFormat {
                irq: 13,
                ret: None,
                ret_symbol: Some("handled".into()),
            }),
        ),
        event(
            1_000_000_014,
            Event::MmVmscanKswapdSleepFormat(p::MmVmscanKswapdSleepFormat { nid: 0 }),
        ),
        event(
            1_000_000_015,
            Event::MmVmscanKswapdWakeFormat(p::MmVmscanKswapdWakeFormat {
                nid: 0,
                zid: None,
                order: 6,
            }),
        ),
        event(
            1_000_000_016,
            Event::RssStatFormat(p::RssStatFormat {
                mm_id: 42,
                curr: 0,
                member: None,
                size: None,
                member_name: Some("MM_FILEPAGES".into()),
                signed_size: Some(4096),
            }),
        ),
        event(
            1_000_000_017,
            Event::SchedSwitchFormat(p::SchedSwitchFormat {
                prev_comm: "worker".into(),
                prev_pid: 7,
                prev_prio: 120,
                prev_state: 1,
                next_comm: "target".into(),
                next_pid: 8,
                next_prio: 100,
            }),
        ),
        event(
            1_000_000_018,
            Event::SchedWakeupFormat(p::SchedWakeupFormat {
                comm: "target".into(),
                pid: 8,
                prio: 120,
                success: None,
                target_cpu: 0,
            }),
        ),
        event(
            1_000_000_019,
            Event::SchedWakeupNewFormat(p::SchedWakeupNewFormat {
                comm: "child".into(),
                pid: 9,
                prio: 120,
                success: None,
                target_cpu: 0,
            }),
        ),
        event(
            1_000_000_020,
            Event::SoftirqEntryFormat(p::SoftirqEntryFormat {
                vec: 9,
                action: Some("RCU".into()),
            }),
        ),
        event(
            1_000_000_021,
            Event::SoftirqExitFormat(p::SoftirqExitFormat {
                vec: 9,
                action: Some("RCU".into()),
            }),
        ),
        event(
            1_000_000_022,
            Event::SoftirqRaiseFormat(p::SoftirqRaiseFormat {
                vec: 9,
                action: Some("RCU".into()),
            }),
        ),
        event(
            1_000_000_023,
            Event::PrintFormat(p::PrintFormat {
                ip: None,
                buf: "B|70|same-source".into(),
            }),
        ),
        event(
            1_000_000_024,
            Event::WorkqueueExecuteEndFormat(p::WorkqueueExecuteEndFormat {
                work: 0xbe11ff42,
                function_symbol: Some("work_fn".into()),
            }),
        ),
        event(
            1_000_000_025,
            Event::WorkqueueExecuteStartFormat(p::WorkqueueExecuteStartFormat {
                work: 0xbe11ff42,
                function: None,
                function_symbol: Some("work_fn".into()),
            }),
        ),
    ]
}

fn same_source_text() -> &'static str {
    concat!(
        "worker-7 ( 70) [000] ..... 1.000000001: binder_transaction: transaction=101 dest_node=0 dest_proc=8 dest_thread=9 reply=1 flags=0x10 code=0x12\n",
        "worker-7 ( 70) [000] ..... 1.000000002: binder_transaction_received: transaction=101\n",
        "worker-7 ( 70) [000] ..... 1.000000003: block_bio_remap: 179,0 WS 200 + 8 <- (179,15) 100\n",
        "worker-7 ( 70) [000] ..... 1.000000004: block_rq_complete: 179,0 WS () 200 + 8 [0]\n",
        "worker-7 ( 70) [000] ..... 1.000000005: block_rq_insert: 179,0 WS 4096 () 200 + 8 [worker]\n",
        "worker-7 ( 70) [000] ..... 1.000000006: block_rq_issue: 179,0 WS 4096 () 200 + 8 [worker]\n",
        "worker-7 ( 70) [000] ..... 1.000000007: cpu_idle: state=4294967295 cpu_id=0\n",
        "worker-7 ( 70) [000] ..... 1.000000008: ipi_entry: (Function call interrupts)\n",
        "worker-7 ( 70) [000] ..... 1.000000009: ipi_exit: (Function call interrupts)\n",
        "worker-7 ( 70) [000] ..... 1.000000010: ipi_raise: target_mask=00000000,00000001 (Function call interrupts)\n",
        "worker-7 ( 70) [000] ..... 1.000000011: ipi_send_cpu: cpu=0 callsite=callsite+0x1/0x2 callback=callback+0x0/0x1\n",
        "worker-7 ( 70) [000] ..... 1.000000012: irq_handler_entry: irq=13 name=arch_timer\n",
        "worker-7 ( 70) [000] ..... 1.000000013: irq_handler_exit: irq=13 ret=handled\n",
        "worker-7 ( 70) [000] ..... 1.000000014: mm_vmscan_kswapd_sleep: nid=0\n",
        "worker-7 ( 70) [000] ..... 1.000000015: mm_vmscan_kswapd_wake: nid=0 order=6\n",
        "worker-7 ( 70) [000] ..... 1.000000016: rss_stat: mm_id=42 curr=0 type=MM_FILEPAGES size=4096B\n",
        "worker-7 ( 70) [000] ..... 1.000000017: sched_switch: prev_comm=worker prev_pid=7 prev_prio=120 prev_state=S ==> next_comm=target next_pid=8 next_prio=100\n",
        "worker-7 ( 70) [000] ..... 1.000000018: sched_wakeup: comm=target pid=8 prio=120 target_cpu=000\n",
        "worker-7 ( 70) [000] ..... 1.000000019: sched_wakeup_new: comm=child pid=9 prio=120 target_cpu=000\n",
        "worker-7 ( 70) [000] ..... 1.000000020: softirq_entry: vec=9 [action=RCU]\n",
        "worker-7 ( 70) [000] ..... 1.000000021: softirq_exit: vec=9 [action=RCU]\n",
        "worker-7 ( 70) [000] ..... 1.000000022: softirq_raise: vec=9 [action=RCU]\n",
        "worker-7 ( 70) [000] ..... 1.000000023: tracing_mark_write: B|70|same-source\n",
        "worker-7 ( 70) [000] ..... 1.000000024: workqueue_execute_end: work struct 00000000be11ff42: function work_fn\n",
        "worker-7 ( 70) [000] ..... 1.000000025: workqueue_execute_start: work struct 00000000be11ff42: function work_fn\n",
    )
}

fn write_htrace(path: &Path) {
    let stats = |status| FtraceCpuStatsMsg {
        status,
        per_cpu_stats: vec![PerCpuStatsMsg {
            cpu: 0,
            ..Default::default()
        }],
        trace_clock: "local".into(),
    };
    let result = TracePluginResult {
        ftrace_cpu_stats: vec![stats(0), stats(1)],
        ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
            cpu: 0,
            event: same_source_events(),
            overwrite: None,
        }],
        ..Default::default()
    };
    let envelope = Envelope {
        name: "ftrace-plugin".into(),
        data: result.encode_to_vec(),
    }
    .encode_to_vec();
    let mut bytes = vec![0; HEADER_SIZE];
    bytes[0..8].copy_from_slice(&HEADER_MAGIC.to_le_bytes());
    bytes[8..16].copy_from_slice(&((HEADER_SIZE + 4 + envelope.len()) as u64).to_le_bytes());
    for (offset, value) in [60, 68, 76, 84, 92, 100].into_iter().zip(1_u64..=6) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(envelope.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&envelope);
    fs::write(path, bytes).unwrap();
}

fn batches(path: &Path) -> Vec<arrow_array::RecordBatch> {
    ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap())
        .unwrap()
        .build()
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn all_supported_same_source_events_have_identical_proto_relations() {
    let root = tempfile::tempdir().unwrap();
    let text_source = root.path().join("same.ftrace");
    let hitrace_source = root.path().join("same.htrace");
    let text_dataset = root.path().join("text");
    let hitrace_dataset = root.path().join("hitrace");
    fs::write(&text_source, same_source_text()).unwrap();
    write_htrace(&hitrace_source);
    import_text_ftrace(
        &text_source,
        TextFtraceClock::Monotonic,
        DatasetWriteTarget::write_to_empty(&text_dataset),
    )
    .unwrap();
    import_hitrace(
        &hitrace_source,
        DatasetWriteTarget::write_to_empty(&hitrace_dataset),
        |_| Ok(()),
    )
    .unwrap();

    let tables = inspect_dataset(&text_dataset)
        .unwrap()
        .tables()
        .iter()
        .map(|table| table.name().to_owned())
        .filter(|name| name.starts_with("trace_plugin_result_ftrace_cpu_detail"))
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 27, "detail + event + 25 payload relations");
    for table in tables {
        let file = format!("{table}.parquet");
        assert_eq!(
            batches(&text_dataset.join("tables").join(&file)),
            batches(&hitrace_dataset.join("tables").join(&file)),
            "same-source mismatch in {table}"
        );
    }
}
