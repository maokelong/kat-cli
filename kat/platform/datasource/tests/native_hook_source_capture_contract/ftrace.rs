use super::*;
use protobuf::{CodedInputStream, UnknownValue};

#[test]
fn ftrace_branch_claims_only_its_two_exact_routes() {
    use formats::hitrace::profiler::PluginEnvelope;
    use native_hook_source::NativeHookSourceCapture;

    let cases = [
        (
            profiler_message(
                "ftrace-plugin",
                proto::TracePluginResult::default().encode_to_vec(),
            ),
            true,
        ),
        (
            profiler_message(
                "ftrace-plugin_config",
                proto::TracePluginConfig::default().encode_to_vec(),
            ),
            true,
        ),
        (profiler_message("ftrace-plugin-extra", vec![0x80]), false),
        (
            profiler_message("ftrace-plugin_config-extra", vec![0x80]),
            false,
        ),
    ];
    let mut capture = NativeHookSourceCapture::new(protobuf_source::SpoolOptions::new(2))
        .expect("Native Hook capture initializes");
    for (index, (message, expected)) in cases.iter().enumerate() {
        let envelope = PluginEnvelope::from_profiler_plugin_data(message, 1_024 + index);
        assert_eq!(
            capture
                .try_claim(&envelope)
                .expect("route dispatch succeeds"),
            *expected,
            "{}",
            message.name
        );
    }
    capture.finish().expect("empty ftrace roots finish");
}

struct RealFtraceWireCensus {
    result_roots: i64,
    config_roots: i64,
    cpu_details: i64,
    events: i64,
    sched_switches: i64,
    irq_handler_entries: i64,
    unknown_result_field_8: usize,
    first_detail_row_id: u64,
    first_detail_events: Value,
    first_sched_switch: Value,
    clocks: Value,
    config_events: Vec<String>,
    config_categories: Vec<String>,
}

fn census_real_ftrace_wire(path: &std::path::Path) -> anyhow::Result<RealFtraceWireCensus> {
    use formats::hitrace::{
        file::{HIPROFILER_PROTOBUF_BIN, read_profiler_section},
        profiler::for_each_profiler_envelope_frame,
    };
    let bytes = std::fs::read(path)?;
    let mut census = RealFtraceWireCensus {
        result_roots: 0,
        config_roots: 0,
        cpu_details: 0,
        events: 0,
        sched_switches: 0,
        irq_handler_entries: 0,
        unknown_result_field_8: 0,
        first_detail_row_id: 0,
        first_detail_events: Value::Null,
        first_sched_switch: Value::Null,
        clocks: json!([]),
        config_events: Vec::new(),
        config_categories: Vec::new(),
    };
    let mut offset = 0;
    while offset < bytes.len() {
        let section = read_profiler_section(&bytes, offset)?;
        if section.header.data_type == HIPROFILER_PROTOBUF_BIN {
            for_each_profiler_envelope_frame(section.body(&bytes), |message, _| {
                match message.name.as_str() {
                    "ftrace-plugin" => {
                        census.result_roots += 1;
                        let root_row_id = u64::try_from(census.result_roots - 1)?;
                        let result_fields = wire_fields(&message.data)?;
                        census.unknown_result_field_8 += result_fields
                            .iter()
                            .filter(|field| field.number == 8)
                            .count();
                        let clocks = result_fields
                            .iter()
                            .filter(|field| field.number == 6)
                            .enumerate()
                            .map(|(index, field)| {
                                let fields = wire_fields(field.bytes()?)?;
                                Ok(json!({
                                    "_kat_parent_row_id": root_row_id,
                                    "_kat_repeated_index": index,
                                    "id": wire_varint(&fields, 1).unwrap_or_default() as i32,
                                    "time": wire_message(&fields, 2).map(wire_time).transpose()?,
                                    "resolution": wire_message(&fields, 3).map(wire_time).transpose()?,
                                }))
                            })
                            .collect::<anyhow::Result<Vec<_>>>()?;
                        if !clocks.is_empty() {
                            census.clocks = Value::Array(clocks);
                        }
                        for detail in result_fields.iter().filter(|field| field.number == 2) {
                            let detail_row_id = u64::try_from(census.cpu_details)?;
                            census.cpu_details += 1;
                            let detail_fields = wire_fields(detail.bytes()?)?;
                            let events = detail_fields.iter().filter(|field| field.number == 2);
                            let mut detail_events = Vec::new();
                            for (index, event) in events.enumerate() {
                                let event_row_id = u64::try_from(census.events)?;
                                census.events += 1;
                                let event_fields = wire_fields(event.bytes()?)?;
                                let common_fields = wire_message(&event_fields, 50)
                                    .map(|bytes| -> anyhow::Result<_> {
                                        let fields = wire_fields(bytes)?;
                                        Ok(json!({
                                            "type": wire_varint(&fields, 1).unwrap_or_default() as u32,
                                            "flags": wire_varint(&fields, 2).unwrap_or_default() as u32,
                                            "preempt_count": wire_varint(&fields, 3).unwrap_or_default() as u32,
                                            "pid": wire_varint(&fields, 4).unwrap_or_default() as u32 as i32,
                                        }))
                                    })
                                    .transpose()?;
                                detail_events.push(json!({
                                    "_kat_parent_row_id": detail_row_id,
                                    "_kat_repeated_index": index,
                                    "timestamp": wire_varint(&event_fields, 1).unwrap_or_default(),
                                    "tgid": wire_varint(&event_fields, 2).unwrap_or_default() as u32 as i32,
                                    "comm": wire_string(&event_fields, 3).unwrap_or_default(),
                                    "common_fields": common_fields,
                                }));
                                if let Some(bytes) = wire_message(&event_fields, 2417) {
                                    census.sched_switches += 1;
                                    if census.first_sched_switch.is_null() {
                                        let fields = wire_fields(bytes)?;
                                        census.first_sched_switch = json!({
                                            "_kat_parent_row_id": event_row_id,
                                            "prev_comm": wire_string(&fields, 1).unwrap_or_default(),
                                            "prev_pid": wire_varint(&fields, 2).unwrap_or_default() as u32 as i32,
                                            "prev_prio": wire_varint(&fields, 3).unwrap_or_default() as u32 as i32,
                                            "prev_state": wire_varint(&fields, 4).unwrap_or_default(),
                                            "next_comm": wire_string(&fields, 5).unwrap_or_default(),
                                            "next_pid": wire_varint(&fields, 6).unwrap_or_default() as u32 as i32,
                                            "next_prio": wire_varint(&fields, 7).unwrap_or_default() as u32 as i32,
                                        });
                                    }
                                }
                                if wire_message(&event_fields, 1500).is_some() {
                                    census.irq_handler_entries += 1;
                                }
                            }
                            if census.first_detail_events.is_null() && !detail_events.is_empty() {
                                census.first_detail_row_id = detail_row_id;
                                census.first_detail_events = Value::Array(detail_events);
                            }
                        }
                    }
                    "ftrace-plugin_config" => {
                        census.config_roots += 1;
                        for field in wire_fields(&message.data)? {
                            match field.number {
                                1 => census.config_events.push(field.string()?.to_owned()),
                                2 => census.config_categories.push(field.string()?.to_owned()),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            })?;
        }
        offset = section.end;
    }
    anyhow::ensure!(
        !census.first_detail_events.is_null(),
        "real ftrace has no event-bearing CPU detail"
    );
    anyhow::ensure!(
        !census.first_sched_switch.is_null(),
        "real ftrace has no sched_switch oneof"
    );
    Ok(census)
}

fn indexed_values(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .enumerate()
            .map(|(index, value)| json!({"_kat_repeated_index": index, "value": value}))
            .collect(),
    )
}

struct WireField {
    number: u64,
    value: WireValue,
}

enum WireValue {
    Varint(u64),
    Fixed64,
    Bytes(Vec<u8>),
    Fixed32,
}

impl WireField {
    fn bytes(&self) -> anyhow::Result<&[u8]> {
        match &self.value {
            WireValue::Bytes(bytes) => Ok(bytes),
            WireValue::Varint(_) | WireValue::Fixed32 | WireValue::Fixed64 => {
                anyhow::bail!("protobuf field {} is not bytes", self.number)
            }
        }
    }

    fn string(&self) -> anyhow::Result<&str> {
        Ok(std::str::from_utf8(self.bytes()?)?)
    }
}

fn wire_fields(bytes: &[u8]) -> anyhow::Result<Vec<WireField>> {
    let mut input = CodedInputStream::from_bytes(bytes);
    let mut fields = Vec::new();
    while let Some(tag) = input.read_raw_tag_or_eof()? {
        let number = u64::from(tag >> 3);
        anyhow::ensure!(number != 0, "protobuf field number is zero");
        let wire_type = protobuf::rt::WireType::new(tag & 7)
            .ok_or_else(|| anyhow::anyhow!("unsupported protobuf wire type {}", tag & 7))?;
        let value = match input.read_unknown(wire_type)? {
            UnknownValue::Varint(value) => WireValue::Varint(value),
            UnknownValue::Fixed64(_) => WireValue::Fixed64,
            UnknownValue::LengthDelimited(value) => WireValue::Bytes(value),
            UnknownValue::Fixed32(_) => WireValue::Fixed32,
        };
        fields.push(WireField { number, value });
    }
    Ok(fields)
}

fn wire_varint(fields: &[WireField], number: u64) -> Option<u64> {
    fields.iter().find_map(|field| {
        (field.number == number).then_some(match &field.value {
            WireValue::Varint(value) => Some(*value),
            WireValue::Bytes(_) | WireValue::Fixed32 | WireValue::Fixed64 => None,
        })?
    })
}

fn wire_message(fields: &[WireField], number: u64) -> Option<&[u8]> {
    fields.iter().find_map(|field| {
        (field.number == number).then_some(match &field.value {
            WireValue::Bytes(bytes) => Some(bytes.as_slice()),
            WireValue::Varint(_) | WireValue::Fixed32 | WireValue::Fixed64 => None,
        })?
    })
}

fn wire_string(fields: &[WireField], number: u64) -> Option<String> {
    wire_message(fields, number)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(str::to_owned)
}

fn wire_time(bytes: &[u8]) -> anyhow::Result<Value> {
    let fields = wire_fields(bytes)?;
    Ok(json!({
        "tv_sec": wire_varint(&fields, 1).unwrap_or_default() as u32,
        "tv_nsec": wire_varint(&fields, 2).unwrap_or_default() as u32,
    }))
}

#[tokio::test]
async fn ftrace_roots_preserve_defined_values_presence_oneof_and_repeated_order()
-> anyhow::Result<()> {
    use proto::kat::hitrace::{
        ClockDetailMsg, FtraceCpuDetailMsg, FtraceCpuStatsMsg, FtraceEvent, IrqHandlerEntryFormat,
        PerCpuStatsMsg, SchedSwitchFormat, TracePluginConfig, TracePluginResult, clock_detail_msg,
        ftrace_cpu_stats_msg, ftrace_event, trace_plugin_config,
    };

    let result = TracePluginResult {
        ftrace_cpu_stats: vec![
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceStart as i32,
                per_cpu_stats: vec![PerCpuStatsMsg {
                    cpu: 7,
                    ..Default::default()
                }],
                trace_clock: "boot".to_owned(),
            },
            FtraceCpuStatsMsg {
                status: ftrace_cpu_stats_msg::Status::TraceEnd as i32,
                per_cpu_stats: vec![PerCpuStatsMsg {
                    cpu: 7,
                    ..Default::default()
                }],
                trace_clock: "boot".to_owned(),
            },
        ],
        ftrace_cpu_detail: vec![FtraceCpuDetailMsg {
            cpu: 7,
            overwrite: Some(0),
            event: vec![
                FtraceEvent {
                    timestamp: 101,
                    tgid: Some(201),
                    comm: "first".to_owned(),
                    common_fields: None,
                    event: Some(ftrace_event::Event::SchedSwitchFormat(SchedSwitchFormat {
                        prev_comm: "before".to_owned(),
                        prev_pid: 301,
                        prev_prio: 11,
                        prev_state: 0x1_0000_0001,
                        next_comm: "after".to_owned(),
                        next_pid: 302,
                        next_prio: 12,
                    })),
                },
                FtraceEvent {
                    timestamp: 102,
                    tgid: Some(202),
                    comm: "second".to_owned(),
                    common_fields: Some(ftrace_event::CommonFileds {
                        r#type: Some(41),
                        flags: Some(42),
                        preempt_count: Some(43),
                        pid: 44,
                    }),
                    event: Some(ftrace_event::Event::IrqHandlerEntryFormat(
                        IrqHandlerEntryFormat {
                            irq: 55,
                            name: "irq-name".to_owned(),
                        },
                    )),
                },
            ],
        }],
        clocks_detail: vec![
            ClockDetailMsg {
                id: clock_detail_msg::ClockId::Boottime as i32,
                time: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 601,
                    tv_nsec: 602,
                }),
                resolution: None,
            },
            ClockDetailMsg {
                id: clock_detail_msg::ClockId::MonotonicRaw as i32,
                time: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 605,
                    tv_nsec: 606,
                }),
                resolution: Some(clock_detail_msg::TimeSpec {
                    tv_sec: 603,
                    tv_nsec: 604,
                }),
            },
        ],
        version: "ftrace-v1".to_owned(),
        ..Default::default()
    };
    let config = TracePluginConfig {
        ftrace_events: vec![
            "sched/sched_switch".to_owned(),
            "irq/irq_handler_entry".to_owned(),
        ],
        hitrace_categories: vec!["sched".to_owned(), "irq".to_owned()],
        hitrace_apps: vec!["app-b".to_owned(), "app-a".to_owned()],
        buffer_size_kb: 8_192,
        clock: "boot".to_owned(),
        parse_mode: trace_plugin_config::ParseMode::RawData as i32,
        ..Default::default()
    };

    let directory = tempdir()?;
    let source_path = directory.path().join("synthetic-ftrace.htrace");
    std::fs::write(
        &source_path,
        profiler_section([
            profiler_message("ftrace-plugin", result.encode_to_vec()),
            profiler_message("ftrace-plugin_config", config.encode_to_vec()),
        ]),
    )?;
    let dataset_path = directory.path().join("dataset");
    crate::import_hitrace(
        &source_path,
        crate::DatasetWriteTarget::write_to_empty(&dataset_path),
        |_| Ok(()),
    )?;
    let context = register_resolved_dataset(&dataset_path).await?;

    assert_eq!(
        query_json(
            &context,
            "select _kat_repeated_index, timestamp, tgid, comm, common_fields \
             from trace_plugin_result_ftrace_cpu_detail_event order by _kat_repeated_index",
        )
        .await,
        json!([
            {"_kat_repeated_index": 0, "timestamp": 101, "tgid": 201, "comm": "first", "common_fields": null},
            {"_kat_repeated_index": 1, "timestamp": 102, "tgid": 202, "comm": "second", "common_fields": {"type": 41, "flags": 42, "preempt_count": 43, "pid": 44}},
        ])
    );
    assert_eq!(
        query_json(&context, "select prev_comm, prev_pid, prev_state, next_comm, next_pid from trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format").await,
        json!([{"prev_comm": "before", "prev_pid": 301, "prev_state": 4_294_967_297_u64, "next_comm": "after", "next_pid": 302}])
    );
    assert_eq!(
        query_json(&context, "select irq, name from trace_plugin_result_ftrace_cpu_detail_event_irq_handler_entry_format").await,
        json!([{"irq": 55, "name": "irq-name"}])
    );
    assert_eq!(
        query_json(&context, "select _kat_repeated_index, id, time, resolution from trace_plugin_result_clocks_detail order by _kat_repeated_index").await,
        json!([
            {"_kat_repeated_index": 0, "id": 1, "time": {"tv_sec": 601, "tv_nsec": 602}, "resolution": null},
            {"_kat_repeated_index": 1, "id": 6, "time": {"tv_sec": 605, "tv_nsec": 606}, "resolution": {"tv_sec": 603, "tv_nsec": 604}},
        ])
    );
    assert_eq!(
        query_json(&context, "select _kat_repeated_index, value from trace_plugin_config_ftrace_events order by _kat_repeated_index").await,
        json!([
            {"_kat_repeated_index": 0, "value": "sched/sched_switch"},
            {"_kat_repeated_index": 1, "value": "irq/irq_handler_entry"},
        ])
    );
    assert_eq!(
        query_json(
            &context,
            "select buffer_size_kb, clock, parse_mode from trace_plugin_config"
        )
        .await,
        json!([{"buffer_size_kb": 8192, "clock": "boot", "parse_mode": 2}])
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires KAT_REAL_FTRACE_SOURCE to name the reviewed real capture"]
async fn formal_import_real_ftrace_matches_independent_wire_census() -> anyhow::Result<()> {
    let path = std::env::var_os("KAT_REAL_FTRACE_SOURCE")
        .ok_or_else(|| anyhow::anyhow!("KAT_REAL_FTRACE_SOURCE is required"))?;
    let wire = census_real_ftrace_wire(std::path::Path::new(&path))?;
    let directory = tempdir()?;
    let dataset_path = directory.path().join("dataset");
    crate::import_hitrace(
        &path,
        crate::DatasetWriteTarget::write_to_empty(&dataset_path),
        |_| Ok(()),
    )?;
    assert_eq!(crate::resolve_dataset(&dataset_path)?.tables().len(), 49);
    assert!(!dataset_path.join("tables/sched_switch.parquet").exists());
    let context = register_resolved_dataset(&dataset_path).await?;

    assert_eq!(wire.unknown_result_field_8, 237);
    for (table, expected) in [
        (
            "profiler_payload_occurrence",
            wire.result_roots + wire.config_roots,
        ),
        ("trace_plugin_result", wire.result_roots),
        ("trace_plugin_config", wire.config_roots),
        ("trace_plugin_result_ftrace_cpu_detail", wire.cpu_details),
        ("trace_plugin_result_ftrace_cpu_detail_event", wire.events),
        (
            "trace_plugin_config_ftrace_events",
            wire.config_events.len() as i64,
        ),
        (
            "trace_plugin_config_hitrace_categories",
            wire.config_categories.len() as i64,
        ),
        (
            "trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format",
            wire.sched_switches,
        ),
        (
            "trace_plugin_result_ftrace_cpu_detail_event_irq_handler_entry_format",
            wire.irq_handler_entries,
        ),
    ] {
        assert_eq!(
            query_json(&context, &format!("select count(*) as rows from {table}")).await,
            json!([{"rows": expected}]),
            "unexpected real ftrace row count for {table}"
        );
    }

    assert_eq!(wire.result_roots, 26_337);
    assert_eq!(wire.config_roots, 1);
    assert_eq!(wire.cpu_details, 26_332);
    assert_eq!(wire.events, 2_474_677);
    assert_eq!(wire.sched_switches, 345_796);
    assert_eq!(wire.irq_handler_entries, 225_879);

    assert_eq!(
        query_json(
            &context,
            "select _kat_repeated_index, value from trace_plugin_config_ftrace_events \
             order by _kat_repeated_index",
        )
        .await,
        indexed_values(&wire.config_events)
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_repeated_index, value from trace_plugin_config_hitrace_categories \
             order by _kat_repeated_index",
        )
        .await,
        indexed_values(&wire.config_categories)
    );
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select _kat_parent_row_id, _kat_repeated_index, timestamp, tgid, comm, common_fields \
                 from trace_plugin_result_ftrace_cpu_detail_event \
                 where _kat_parent_row_id = {} order by _kat_repeated_index",
                wire.first_detail_row_id
            ),
        )
        .await,
        wire.first_detail_events
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, prev_comm, prev_pid, prev_prio, prev_state, \
                    next_comm, next_pid, next_prio \
             from trace_plugin_result_ftrace_cpu_detail_event_sched_switch_format \
             order by _kat_parent_row_id limit 1",
        )
        .await,
        json!([wire.first_sched_switch])
    );
    assert_eq!(
        query_json(
            &context,
            "select _kat_parent_row_id, _kat_repeated_index, id, time, resolution \
             from trace_plugin_result_clocks_detail \
             order by _kat_parent_row_id, _kat_repeated_index",
        )
        .await,
        wire.clocks
    );

    Ok(())
}
