use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path))
        .unwrap_or_else(|error| panic!("{path} can be read: {error}"))
}

fn joined_sources(paths: &[&str]) -> String {
    paths
        .iter()
        .map(|path| source(path))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn datasource_tests_live_under_tests_directory() {
    let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    let forbidden_markers = ["#[cfg(test)]", "#[test]", "#[tokio::test]"];

    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(&path)
                .unwrap_or_else(|error| panic!("{} can be listed: {error}", path.display()))
            {
                stack.push(
                    entry
                        .unwrap_or_else(|error| {
                            panic!("{} entry can be read: {error}", path.display())
                        })
                        .path(),
                );
            }
            continue;
        }

        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }

        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} can be read: {error}", path.display()));
        for marker in forbidden_markers {
            assert!(
                !source.contains(marker),
                "{} should keep tests under tests/",
                path.display()
            );
        }
    }
}

#[test]
fn datasource_uses_reviewer_layer_boundaries() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    for path in [
        "src/formats/mod.rs",
        "src/formats/hitrace/mod.rs",
        "src/formats/hitrace/file.rs",
        "src/formats/hitrace/profiler/mod.rs",
        "src/formats/hitrace/profiler/envelope.rs",
        "src/formats/hitrace/profiler/framing.rs",
        "src/formats/hitrace/profiler/payload.rs",
        "src/formats/hitrace/profiler/registry.rs",
        "src/domains/mod.rs",
        "src/domains/fixed_result/mod.rs",
        "src/domains/ftrace/mod.rs",
        "src/domains/ftrace/event.rs",
        "src/domains/ftrace/packet.rs",
        "src/domains/native_hook/mod.rs",
        "src/domains/native_hook/event.rs",
        "src/domains/native_hook/packet.rs",
        "src/sinks/mod.rs",
        "src/sinks/arrow/mod.rs",
        "src/sinks/arrow/native_hook.rs",
        "src/sinks/arrow/ftrace.rs",
        "src/sinks/arrow/table.rs",
        "src/arrow_table.rs",
        "src/record.rs",
    ] {
        assert!(
            std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should exist"
        );
    }

    for path in [
        "src/hitrace.rs",
        "src/ftrace.rs",
        "src/ftrace/mod.rs",
        "src/plugin_flow/mod.rs",
        "src/plugin_flow/envelope.rs",
        "src/plugin_flow/registry.rs",
        "src/plugin_flow/segment.rs",
        "src/formats/hitrace/profiler/segment.rs",
        "src/sinks/arrow/event_table.rs",
        "src/sinks/arrow/table_builder.rs",
    ] {
        assert!(
            !std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should not remain as a datasource center"
        );
    }
}

#[test]
fn hitrace_file_adapter_does_not_decode_plugins_or_write_arrow() {
    let hitrace_file = source("src/formats/hitrace/file.rs");

    for marker in [
        "domains::ftrace",
        "domains::fixed_result",
        "domains::native_hook",
        "FTRACE_PLUGIN_NAME",
        "nativehook",
        "hookdaemon",
        "NativeHook",
        "decode_plugin_payload",
        "ProfilerPluginData",
        "TraceRecord::ProfilerSection",
        "TracePluginResult::decode",
        "SchedDirectTableBuilders",
        "ArrayBuilder",
        "RecordBatch",
        "MemTable",
        "thread_state",
        "sched_slice",
        "raw_event",
    ] {
        assert!(
            !hitrace_file.contains(marker),
            "{marker} should not live in formats/hitrace/file"
        );
    }
}

#[test]
fn profiler_envelope_mechanism_does_not_own_domain_decoders() {
    let profiler_sources = joined_sources(&[
        "src/formats/hitrace/profiler/mod.rs",
        "src/formats/hitrace/profiler/envelope.rs",
        "src/formats/hitrace/profiler/framing.rs",
        "src/formats/hitrace/profiler/payload.rs",
        "src/formats/hitrace/profiler/registry.rs",
    ]);

    assert!(profiler_sources.contains("PluginEnvelopeKind"));
    assert!(profiler_sources.contains("trait PluginDecoder"));
    assert!(profiler_sources.contains("fn configure("));
    assert!(profiler_sources.contains("fn decode_data("));
    assert!(profiler_sources.contains("fn finish("));
    assert!(profiler_sources.contains("PluginDecoderSpec"));
    assert!(profiler_sources.contains("PluginPayloadRegistry"));
    assert!(profiler_sources.contains("ProfilerPluginData"));
    assert!(profiler_sources.contains("for_each_profiler_envelope_frame"));
    assert!(!profiler_sources.contains("for_each_len_prefixed_message"));
    assert!(profiler_sources.contains("decode_payload"));

    for marker in [
        "domains::ftrace",
        "domains::native_hook",
        "TracePluginResult",
        "CpuData",
        "MemoryData",
        "ProcessData",
        "DiskioData",
        "NetworkDatas",
        "GpuData",
        "BatchNativeHookData",
        "NativeHookConfig",
        "FTRACE_PLUGIN_DECODER",
        "FIXED_RESULT_PLUGIN_DECODERS",
        "NATIVE_HOOK_PLUGIN_DECODER",
        "HOOK_DAEMON_PLUGIN_DECODER",
        "DecodePluginPayload",
        "ArrayBuilder",
        "RecordBatch",
        "MemTable",
    ] {
        assert!(
            !profiler_sources.contains(marker),
            "{marker} should not live in the profiler envelope mechanism"
        );
    }
}

#[test]
fn hitrace_pipeline_assembles_profiler_decoder_specs() {
    let pipeline = source("src/formats/hitrace/mod.rs");

    for marker in [
        "FTRACE_PLUGIN_DECODER",
        "FIXED_RESULT_PLUGIN_DECODERS",
        "NATIVE_HOOK_PLUGIN_DECODER",
        "HOOK_DAEMON_PLUGIN_DECODER",
        "PluginPayloadRegistry::new",
    ] {
        assert!(
            pipeline.contains(marker),
            "{marker} should be assembled by the hitrace pipeline"
        );
    }
}

#[test]
fn ftrace_domain_decodes_payload_to_neutral_records() {
    let ftrace_sources = joined_sources(&[
        "src/domains/ftrace/mod.rs",
        "src/domains/ftrace/event.rs",
        "src/domains/ftrace/packet.rs",
    ]);

    assert!(ftrace_sources.contains("pub(crate) enum FtraceRecord"));

    for marker in [
        "SchedDirectTableBuilders",
        "DirectEventTableBuilder",
        "ArrayBuilder",
        "RecordBatch",
    ] {
        assert!(
            !ftrace_sources.contains(marker),
            "{marker} should not live in domains/ftrace"
        );
    }
}

#[test]
fn arrow_sink_owns_record_to_table_conversion() {
    let sink_mod = source("src/sinks/arrow/mod.rs");
    let ftrace_sink = source("src/sinks/arrow/ftrace.rs");
    let native_hook_sink = source("src/sinks/arrow/native_hook.rs");
    let table = source("src/sinks/arrow/table.rs");
    let native_hook_domain = joined_sources(&[
        "src/domains/native_hook/mod.rs",
        "src/domains/native_hook/event.rs",
        "src/domains/native_hook/packet.rs",
    ]);
    let fixed_result_domain = source("src/domains/fixed_result/mod.rs");
    let generated_native_hook_records =
        fs::read_to_string(format!("{}/native_hook_records.rs", env!("OUT_DIR")))
            .expect("generated native hook records can be read");
    let generated_fixed_result_records =
        fs::read_to_string(format!("{}/fixed_result_records.rs", env!("OUT_DIR")))
            .expect("generated fixed result records can be read");
    let generated_fixed_result_builders = fs::read_to_string(format!(
        "{}/fixed_result_table_builders.rs",
        env!("OUT_DIR")
    ))
    .expect("generated fixed result table builders can be read");
    let record = source("src/record.rs");
    let arrow_table = source("src/arrow_table.rs");

    assert!(!arrow_table.contains("TraceRecord"));
    assert!(!arrow_table.contains("TraceRecordSink"));
    assert!(!arrow_table.contains("ProfilerSection"));
    assert!(!sink_mod.contains("ProfilerSection"));
    assert!(!sink_mod.contains("profiler_table_seen"));
    assert!(!sink_mod.contains("profiler_rows"));
    assert!(!sink_mod.contains("SchedDirectTableBuilders"));
    assert!(!sink_mod.contains("mod event_table"));
    assert!(sink_mod.contains("mod ftrace"));
    assert!(sink_mod.contains("mod native_hook"));
    assert!(sink_mod.contains("FtraceTableSet"));
    assert!(sink_mod.contains("NativeHookTableSet"));
    assert!(sink_mod.contains("FixedResultTableSet"));
    assert!(table.contains("pub(crate) struct MessageTableBuilder"));
    assert!(!table.contains("pub(crate) struct TableBuilder"));
    assert!(table.contains("pub(crate) struct EventTableBuilder"));
    assert!(table.contains("pub(crate) struct EventTableRow"));
    assert!(table.contains("ArrayBuilder"));
    assert!(!table.contains("FtraceEventRecord"));
    assert!(!table.contains("EventMeta"));
    assert!(!table.contains("NativeHookEvent"));
    assert!(ftrace_sink.contains("pub(crate) struct FtraceEventTableBuilder"));
    assert!(!ftrace_sink.contains("DirectEventTableBuilder"));
    assert!(ftrace_sink.contains("pub(crate) struct EventMeta"));
    assert!(ftrace_sink.contains("FtraceEventRecord"));
    assert!(ftrace_sink.contains("EventTableBuilder<EventMeta>"));
    assert!(!ftrace_sink.contains("pub(crate) struct EventRow"));
    assert!(native_hook_sink.contains("pub(crate) struct NativeHookEventMeta"));
    assert!(native_hook_sink.contains("pub(crate) struct NativeHookEventTableBuilder"));
    assert!(native_hook_sink.contains("EventTableBuilder<NativeHookEventMeta>"));
    assert!(!native_hook_sink.contains("pub(crate) struct NativeHookEventRow"));
    assert!(!native_hook_sink.contains("pub(crate) struct NativeHookTableSet"));
    assert!(!native_hook_sink.contains("struct AllocRow"));
    assert!(!native_hook_sink.contains("struct TraceAllocRow"));
    assert!(!native_hook_sink.contains("NativeHookRecord"));
    assert!(!native_hook_sink.contains("NativeHookData"));
    assert!(!native_hook_sink.contains("native_hook_data::Event"));
    assert!(!native_hook_sink.contains("Event::AllocEvent"));
    assert!(!native_hook_sink.contains("Event::MapsInfo"));
    assert!(!native_hook_domain.contains("pub(crate) enum NativeHookRecord"));
    assert!(!native_hook_domain.contains("native_hook_data::Event"));
    assert!(!native_hook_domain.contains("Event::AllocEvent"));
    assert!(!native_hook_domain.contains("Event::MapsInfo"));
    assert!(fixed_result_domain.contains("fixed_result_records.rs"));
    assert!(!fixed_result_domain.contains("CpuData"));
    assert!(!fixed_result_domain.contains("MemoryData"));
    assert!(generated_native_hook_records.contains("pub(crate) enum NativeHookRecord"));
    assert!(generated_native_hook_records.contains("native_hook_data::Event"));
    assert!(generated_native_hook_records.contains("Event::AllocEvent"));
    assert!(generated_native_hook_records.contains("Event::MapsInfo"));
    assert!(generated_native_hook_records.contains("NativeHookRecord::MapsInfo"));
    assert!(generated_native_hook_records.contains("NativeHookRecord::SymbolTable"));
    assert!(generated_fixed_result_records.contains("pub(crate) enum FixedResultRecord"));
    assert!(generated_fixed_result_records.contains("CpuData(Box<CpuData>)"));
    assert!(generated_fixed_result_records.contains("MemoryData(Box<MemoryData>)"));
    assert!(generated_fixed_result_records.contains("NetworkDatas(Box<NetworkDatas>)"));
    assert!(generated_fixed_result_builders.contains("pub(crate) struct FixedResultTableSet"));
    assert!(generated_fixed_result_builders.contains("MessageTableBuilder<CpuData>"));
    assert!(generated_fixed_result_builders.contains("MessageTableBuilder<MemoryData>"));
    assert!(generated_fixed_result_builders.contains("\"gpu_data\""));
    assert!(record.contains("Ftrace(Box<FtraceRecord>)"));
    assert!(!record.contains("FtraceEvent("));
    assert!(record.contains("NativeHook(Box<NativeHookRecord>)"));
    assert!(!record.contains("NativeHookConfig("));
    assert!(!record.contains("NativeHookEvent("));
    assert!(record.contains("FixedResult(Box<FixedResultRecord>)"));
    assert!(!record.contains("CpuData("));
}

#[test]
fn native_hook_table_builders_are_generated_from_oneof_mapping() {
    let build_rs = source("build.rs");
    let generated_builders =
        fs::read_to_string(format!("{}/native_hook_table_builders.rs", env!("OUT_DIR")))
            .expect("generated native hook table builders can be read");

    assert!(!build_rs.contains("const NATIVE_HOOK_EVENT_TABLES"));
    assert!(generated_builders.contains("pub(crate) struct NativeHookTableSet"));
    assert!(!generated_builders.contains("pub(crate) struct NativeHookTableBuilders"));
    assert!(generated_builders.contains("native_hook_trace_alloc"));
    assert!(generated_builders.contains("NativeHookRecord::TraceAlloc"));
    assert!(generated_builders.contains("NativeHookEventTableBuilder::new::<TraceAllocEvent>"));
    assert!(generated_builders.contains("native_hook_maps_info"));
    assert!(generated_builders.contains("NativeHookRecord::MapsInfo"));
    assert!(generated_builders.contains("NativeHookEventTableBuilder::new::<MapsInfo>"));
    assert!(generated_builders.contains("native_hook_symbol_table"));
    assert!(generated_builders.contains("NativeHookRecord::SymbolTable"));
    assert!(generated_builders.contains("NativeHookEventTableBuilder::new::<SymbolTable>"));
    assert!(generated_builders.contains("MessageTableBuilder<NativeHookConfig>"));
    assert!(!generated_builders.contains("config: TableBuilder<NativeHookConfig>"));
    assert!(!generated_builders.contains("struct AllocRow"));
    assert!(!generated_builders.contains("struct TraceAllocRow"));
}

#[test]
fn query_consumes_arrow_table_set() {
    let query = source("src/query.rs");

    for marker in ["load_hitrace_tables", "HITRACE_TABLE", "FtraceTables"] {
        assert!(
            !query.contains(marker),
            "{marker} should not be consumed directly by query"
        );
    }
}

#[test]
fn hitrace_dataset_materializer_uses_streaming_sink_not_arrow_table_set() {
    let materializer = source("src/materializer.rs");

    assert!(materializer.contains("impl TraceRecordSink for HitraceDatasetSink"));
    assert!(materializer.contains("HITRACE_DATASET_FLUSH_RECORDS"));

    for marker in ["ArrowTableSet", "decode_hitrace", "write_hitrace_tables"] {
        assert!(
            !materializer.contains(marker),
            "{marker} should not remain in .htrace dataset materialize"
        );
    }
}

#[test]
fn build_script_splits_codegen_by_responsibility() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let build_rs = source("build.rs");

    for path in [
        "build/proto_codegen.rs",
        "build/ftrace_arrow_codegen.rs",
        "build/native_hook_domain_codegen.rs",
        "build/native_hook_arrow_codegen.rs",
        "build/fixed_result_domain_codegen.rs",
        "build/fixed_result_arrow_codegen.rs",
    ] {
        assert!(
            std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should exist"
        );
    }

    for path in [
        "build/proto_parse.rs",
        "build/ftrace_codegen.rs",
        "build/native_hook_codegen.rs",
    ] {
        assert!(
            !std::path::Path::new(&format!("{manifest_dir}/{path}")).exists(),
            "{path} should be removed after splitting build code by served layer"
        );
    }

    for module in [
        "mod ftrace_arrow_codegen;",
        "mod native_hook_arrow_codegen;",
        "mod native_hook_domain_codegen;",
        "mod fixed_result_arrow_codegen;",
        "mod fixed_result_domain_codegen;",
        "mod proto_codegen;",
    ] {
        assert!(build_rs.contains(module), "{module} should be declared");
    }

    for marker in [
        "mod ftrace_codegen;",
        "mod native_hook_codegen;",
        "mod proto_parse;",
        "fn parse_proto_messages",
        "fs::read_to_string",
        "fn render_ftrace_event_table_builders",
        "fn render_native_hook_table_builders",
        "fn render_native_hook_records",
    ] {
        assert!(
            !build_rs.contains(marker),
            "{marker} should live in a focused build helper"
        );
    }

    assert!(
        build_rs.contains(".load_fds(&proto_files, &[\"proto\"])"),
        "build.rs should load descriptor data before custom codegen"
    );
    assert!(
        build_rs.contains(".compile_fds(fds)"),
        "build.rs should compile the same descriptor data after custom codegen"
    );
    assert!(
        build_rs.contains("PROFILER_ENVELOPE_PROTO_FILES"),
        "build.rs should keep profiler envelope proto inputs explicit"
    );
    assert!(
        build_rs.contains("proto/profiler/profiler_plugin_data.proto"),
        "ProfilerPluginData should be sourced from profiler/profiler_plugin_data.proto"
    );
    assert!(
        !build_rs.contains("proto/hitrace.proto"),
        "ProfilerPluginData should not remain in proto/hitrace.proto"
    );
    assert!(
        !build_rs.contains("proto/services/"),
        "offline datasource should not source profiler envelope from service/RPC proto paths"
    );
}

#[test]
fn ftrace_event_family_generation_avoids_old_sched_entrypoint() {
    let build_rs = source("build.rs");
    let ftrace_arrow_codegen = source("build/ftrace_arrow_codegen.rs");
    let native_hook_domain_codegen = source("build/native_hook_domain_codegen.rs");
    let native_hook_arrow_codegen = source("build/native_hook_arrow_codegen.rs");
    let build_script_sources = format!(
        "{build_rs}\n{ftrace_arrow_codegen}\n{native_hook_domain_codegen}\n{native_hook_arrow_codegen}"
    );
    let generated_builders = fs::read_to_string(format!(
        "{}/ftrace_event_table_builders.rs",
        env!("OUT_DIR")
    ))
    .expect("generated ftrace event table builders can be read");

    for marker in [
        "crate::ftrace",
        "FtraceTable,",
        "SchedDirectTableBuilders",
        "FtraceEvent,",
        "push_event(&mut self, cpu: u32",
        "EventMeta::from_event(cpu, &event)",
        "rows_file",
        "render_sched_rows",
        "SchedSwitchRow",
    ] {
        assert!(
            !generated_builders.contains(marker) && !build_script_sources.contains(marker),
            "{marker} should not remain in generated table builder plumbing"
        );
    }

    assert!(native_hook_domain_codegen.contains("proto/native_hook/native_hook_config.proto"));
    assert!(native_hook_domain_codegen.contains("proto/native_hook/native_hook_result.proto"));
    assert!(
        build_rs.contains("FIXED_RESULT_PROTO_FILES"),
        "fixed result proto inputs should be explicit"
    );
    assert!(
        build_rs.contains("generate_fixed_result_records"),
        "fixed result domain codegen should be wired"
    );
    assert!(
        build_rs.contains("generate_fixed_result_table_builders"),
        "fixed result Arrow codegen should be wired"
    );

    let ftrace_family_decl = ftrace_arrow_codegen
        .split("FTRACE_EVENT_FAMILIES")
        .nth(1)
        .and_then(|source| source.split("pub(crate) struct EventFamilySpec").next())
        .expect("FTRACE_EVENT_FAMILIES declaration can be found");
    assert!(
        !ftrace_family_decl.contains("native_hook"),
        "native hook protos should not be ftrace event families"
    );
}
