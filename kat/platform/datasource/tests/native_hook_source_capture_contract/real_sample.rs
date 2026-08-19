use super::*;

#[tokio::test]
#[ignore = "requires KAT_REAL_NATIVE_HOOK_HITRACE to name a real Native Hook capture"]
async fn real_native_hook_capture_matches_independent_typed_census() {
    use native_hook_source::NativeHookSourceCapture;
    use protobuf_source::SpoolOptions;

    let source = std::path::PathBuf::from(
        std::env::var_os("KAT_REAL_NATIVE_HOOK_HITRACE")
            .expect("set KAT_REAL_NATIVE_HOOK_HITRACE to a real Native Hook capture"),
    );
    let bytes = std::fs::read(&source).expect("real Native Hook capture reads");
    let mut capture =
        NativeHookSourceCapture::new(SpoolOptions::with_limits(8_192, 16 * 1024 * 1024))
            .expect("dormant Native Hook capture is valid");
    let census = census_and_claim_real_native_hook(&bytes, &mut capture)
        .expect("real Native Hook payloads decode and are claimed");

    assert!(
        census.data_roots > 0,
        "sample must contain Native Hook data"
    );
    assert!(
        census.config_roots > 0,
        "sample must contain Native Hook config"
    );
    assert!(census.events > 0, "sample must contain Native Hook events");
    assert_eq!(
        census.claimed_envelopes,
        census.data_roots + census.config_roots,
        "every independently recognized Native Hook envelope must be claimed"
    );

    let prepared = capture
        .finish()
        .expect("real Native Hook config admits all eventful payloads");
    let directory = tempdir().expect("temporary Dataset directory is created");
    let dataset_path = directory.path().join("dataset");
    publish_prepared(prepared, &dataset_path);
    let context = register_resolved_dataset(&dataset_path)
        .await
        .expect("real Native Hook tables register in DataFusion");
    let actual_tables = crate::resolve_dataset(&dataset_path)
        .expect("real Native Hook Dataset resolves")
        .tables()
        .iter()
        .map(|table| table.name().to_string())
        .collect::<std::collections::BTreeSet<_>>();

    assert_table_count(
        &context,
        "profiler_payload_occurrence",
        census.claimed_envelopes,
    )
    .await;
    assert_table_count(&context, "batch_native_hook_data", census.data_roots).await;
    assert_table_count(&context, "native_hook_config", census.config_roots).await;
    assert_table_count(&context, "batch_native_hook_data_events", census.events).await;
    for (&table, &expected) in &census.relation_rows {
        if expected == 0 {
            assert!(
                !actual_tables.contains(table),
                "zero-row relation {table:?} must not be published"
            );
        } else {
            assert_table_count(&context, table, expected).await;
        }
    }
    assert_eq!(
        query_json(
            &context,
            "select 'data' as kind, root._kat_row_id as root_id, \
             root._kat_parent_row_id as occurrence_id, occurrence.envelope_name \
             from batch_native_hook_data root \
             join profiler_payload_occurrence occurrence \
             on root._kat_parent_row_id = occurrence._kat_row_id \
             union all \
             select 'config' as kind, root._kat_row_id as root_id, \
             root._kat_parent_row_id as occurrence_id, occurrence.envelope_name \
             from native_hook_config root \
             join profiler_payload_occurrence occurrence \
             on root._kat_parent_row_id = occurrence._kat_row_id \
             order by occurrence_id",
        )
        .await,
        Value::Array(census.root_links.clone()),
        "root rows must retain their exact source occurrences"
    );

    let mut expected_events = census
        .representative_events
        .values()
        .cloned()
        .collect::<Vec<_>>();
    expected_events.sort_by_key(|row| row["row_id"].as_u64());
    let event_ids = expected_events
        .iter()
        .map(|row| {
            row["row_id"]
                .as_u64()
                .expect("event row id is UInt64")
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select _kat_row_id as row_id, _kat_parent_row_id as root_id, \
                 _kat_repeated_index as repeated_index, tv_sec, tv_nsec \
                 from batch_native_hook_data_events \
                 where _kat_row_id in ({event_ids}) order by _kat_row_id"
            ),
        )
        .await,
        Value::Array(expected_events),
        "one representative of every present event variant must retain value, parent, and order"
    );
    for (&table, expected) in &census.representative_variant_links {
        assert_eq!(
            query_json(
                &context,
                &format!(
                    "select _kat_parent_row_id as event_id from {table} \
                     order by _kat_parent_row_id limit 1"
                ),
            )
            .await,
            Value::Array(vec![expected.clone()]),
            "representative variant in {table:?} must retain its event parent"
        );
    }
    let expected_stack_ips = census
        .representative_stack_ips
        .as_ref()
        .expect("real sample must contain one StackMap with IP values");
    let stack_id = expected_stack_ips[0]["stack_id"]
        .as_u64()
        .expect("StackMap row id is UInt64");
    assert_eq!(
        query_json(
            &context,
            &format!(
                "select _kat_parent_row_id as stack_id, \
                 _kat_repeated_index as repeated_index, value \
                 from batch_native_hook_data_events_stack_map_ip \
                 where _kat_parent_row_id = {stack_id} order by _kat_repeated_index"
            ),
        )
        .await,
        Value::Array(expected_stack_ips.clone()),
        "representative StackMap IP values must retain parent and repeated order"
    );

    eprintln!(
        "real Native Hook census: file={} bytes={} envelopes={} data_roots={} config_roots={} events={} relations={:?}",
        source.display(),
        bytes.len(),
        census.claimed_envelopes,
        census.data_roots,
        census.config_roots,
        census.events,
        census.relation_rows
    );
}

#[derive(Debug)]
struct RealNativeHookCensus {
    claimed_envelopes: u64,
    data_roots: u64,
    config_roots: u64,
    events: u64,
    relation_rows: std::collections::BTreeMap<&'static str, u64>,
    root_links: Vec<Value>,
    representative_events: std::collections::BTreeMap<&'static str, Value>,
    representative_variant_links: std::collections::BTreeMap<&'static str, Value>,
    representative_stack_ips: Option<Vec<Value>>,
}

impl Default for RealNativeHookCensus {
    fn default() -> Self {
        let relation_rows = native_hook_relation_names()
            .into_iter()
            .filter(|name| {
                !matches!(
                    *name,
                    "batch_native_hook_data"
                        | "batch_native_hook_data_events"
                        | "native_hook_config"
                )
            })
            .map(|name| (name, 0))
            .collect();
        Self {
            claimed_envelopes: 0,
            data_roots: 0,
            config_roots: 0,
            events: 0,
            relation_rows,
            root_links: Vec::new(),
            representative_events: std::collections::BTreeMap::new(),
            representative_variant_links: std::collections::BTreeMap::new(),
            representative_stack_ips: None,
        }
    }
}

fn census_and_claim_real_native_hook(
    bytes: &[u8],
    capture: &mut native_hook_source::NativeHookSourceCapture,
) -> anyhow::Result<RealNativeHookCensus> {
    use formats::hitrace::{
        file::{HIPROFILER_PROTOBUF_BIN, PROFILER_HEADER_SIZE, read_profiler_section},
        profiler::{PluginEnvelope, PluginEnvelopeKind, for_each_profiler_envelope_frame},
    };
    let mut census = RealNativeHookCensus::default();
    let mut offset = 0;
    while offset < bytes.len() {
        let section = read_profiler_section(bytes, offset)?;
        if section.header.data_type == HIPROFILER_PROTOBUF_BIN {
            for_each_profiler_envelope_frame(section.body(bytes), |message, frame_offset| {
                let envelope = PluginEnvelope::from_profiler_plugin_data(
                    &message,
                    section.start + PROFILER_HEADER_SIZE + frame_offset,
                );
                let recognized = match (envelope.envelope_name, envelope.kind) {
                    ("nativehook" | "hookdaemon", PluginEnvelopeKind::Data) => {
                        let batch = proto::BatchNativeHookData::decode(message.data.as_slice())?;
                        let root_id = census.data_roots;
                        census.root_links.push(json!({
                            "kind": "data",
                            "root_id": root_id,
                            "occurrence_id": census.claimed_envelopes,
                            "envelope_name": envelope.envelope_name,
                        }));
                        census.data_roots += 1;
                        for (repeated_index, event) in batch.events.iter().enumerate() {
                            let event_row_id = census.events;
                            census.events += 1;
                            observe_real_native_hook_event(
                                &mut census,
                                event,
                                root_id,
                                event_row_id,
                                repeated_index,
                            );
                        }
                        true
                    }
                    ("nativehook_config" | "hookdaemon_config", PluginEnvelopeKind::Config) => {
                        let config = proto::NativeHookConfig::decode(message.data.as_slice())?;
                        census.root_links.push(json!({
                            "kind": "config",
                            "root_id": census.config_roots,
                            "occurrence_id": census.claimed_envelopes,
                            "envelope_name": envelope.envelope_name,
                        }));
                        census.config_roots += 1;
                        add_census_rows(
                            &mut census,
                            "native_hook_config_expand_pids",
                            config.expand_pids.len(),
                        );
                        add_census_rows(
                            &mut census,
                            "native_hook_config_restrace_tag",
                            config.restrace_tag.len(),
                        );
                        true
                    }
                    _ => false,
                };
                let claimed = capture.try_claim(&envelope)?;
                anyhow::ensure!(
                    claimed == recognized,
                    "dormant claim result diverges from the independent route census for {:?}",
                    envelope.envelope_name
                );
                if claimed {
                    census.claimed_envelopes += 1;
                }
                Ok(())
            })?;
        }
        offset = section.end;
    }
    Ok(census)
}

fn observe_real_native_hook_event(
    census: &mut RealNativeHookCensus,
    event: &proto::kat::native_hook::NativeHookData,
    root_id: u64,
    event_row_id: u64,
    repeated_index: usize,
) {
    use proto::kat::native_hook::native_hook_data::Event;

    let Some(event_value) = event.event.as_ref() else {
        return;
    };
    let repeated_index = u64::try_from(repeated_index).expect("event index fits UInt64");
    let observe_variant = |census: &mut RealNativeHookCensus, table: &'static str| {
        let variant_row_id = census.relation_rows[table];
        add_census_rows(census, table, 1);
        census
            .representative_events
            .entry(table)
            .or_insert_with(|| {
                json!({
                    "row_id": event_row_id,
                    "root_id": root_id,
                    "repeated_index": repeated_index,
                    "tv_sec": event.tv_sec,
                    "tv_nsec": event.tv_nsec,
                })
            });
        census
            .representative_variant_links
            .entry(table)
            .or_insert_with(|| json!({ "event_id": event_row_id }));
        variant_row_id
    };

    match event_value {
        Event::AllocEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_alloc_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_alloc_event_frame_info",
                value.frame_info.len(),
            );
        }
        Event::FreeEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_free_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_free_event_frame_info",
                value.frame_info.len(),
            );
        }
        Event::MmapEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_mmap_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_mmap_event_frame_info",
                value.frame_info.len(),
            );
        }
        Event::MunmapEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_munmap_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_munmap_event_frame_info",
                value.frame_info.len(),
            );
        }
        Event::TagEvent(_) => {
            observe_variant(census, "batch_native_hook_data_events_tag_event");
        }
        Event::FilePath(_) => {
            observe_variant(census, "batch_native_hook_data_events_file_path");
        }
        Event::SymbolName(_) => {
            observe_variant(census, "batch_native_hook_data_events_symbol_name");
        }
        Event::ThreadNameMap(_) => {
            observe_variant(census, "batch_native_hook_data_events_thread_name_map");
        }
        Event::MapsInfo(_) => {
            observe_variant(census, "batch_native_hook_data_events_maps_info");
        }
        Event::SymbolTab(_) => {
            observe_variant(census, "batch_native_hook_data_events_symbol_tab");
        }
        Event::FrameMap(_) => {
            observe_variant(census, "batch_native_hook_data_events_frame_map");
        }
        Event::StackMap(value) => {
            let variant_row_id = observe_variant(census, "batch_native_hook_data_events_stack_map");
            add_census_rows(
                census,
                "batch_native_hook_data_events_stack_map_frame_map_id",
                value.frame_map_id.len(),
            );
            add_census_rows(
                census,
                "batch_native_hook_data_events_stack_map_ip",
                value.ip.len(),
            );
            if census.representative_stack_ips.is_none() && !value.ip.is_empty() {
                census.representative_stack_ips = Some(
                    value
                        .ip
                        .iter()
                        .enumerate()
                        .map(|(index, value)| {
                            json!({
                                "stack_id": variant_row_id,
                                "repeated_index": index,
                                "value": value,
                            })
                        })
                        .collect(),
                );
            }
        }
        Event::StatisticsEvent(_) => {
            observe_variant(census, "batch_native_hook_data_events_statistics_event");
        }
        Event::TraceAllocEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_trace_alloc_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_trace_alloc_event_frame_info",
                value.frame_info.len(),
            );
        }
        Event::TraceFreeEvent(value) => {
            observe_variant(census, "batch_native_hook_data_events_trace_free_event");
            add_census_rows(
                census,
                "batch_native_hook_data_events_trace_free_event_frame_info",
                value.frame_info.len(),
            );
        }
    }
}

fn add_census_rows(census: &mut RealNativeHookCensus, relation: &'static str, rows: usize) {
    let rows = u64::try_from(rows).expect("relation row count fits UInt64");
    *census
        .relation_rows
        .get_mut(relation)
        .unwrap_or_else(|| panic!("census relation {relation:?} is declared")) += rows;
}

async fn assert_table_count(context: &SessionContext, table: &str, expected: u64) {
    assert_eq!(
        query_json(context, &format!("select count(*) as count from {table}"),).await,
        json!([{ "count": expected }]),
        "unexpected row count for {table:?}"
    );
}
