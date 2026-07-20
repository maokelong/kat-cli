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

fn path_exists(path: &str) -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(path)
        .exists()
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
fn datasource_uses_relational_layer_boundaries_without_old_arrow_sink() {
    for path in [
        "src/formats/hitrace/mod.rs",
        "src/formats/hitrace/file.rs",
        "src/formats/hitrace/profiler/mod.rs",
        "src/decode/mod.rs",
        "src/decode/profiler/mod.rs",
        "src/decode/profiler/fixed_result/mod.rs",
        "src/decode/profiler/ftrace/mod.rs",
        "src/decode/profiler/native_hook/mod.rs",
        "src/record.rs",
        "src/relational/mod.rs",
        "src/relational/descriptor.rs",
        "src/relational/plan.rs",
        "src/relational/plan_exec.rs",
        "src/relational/sink.rs",
        "src/relational/table_batch.rs",
        "src/relational/table_data.rs",
        "src/dataset/writer.rs",
        "src/dataset/reader.rs",
    ] {
        assert!(path_exists(path), "{path} should exist");
    }

    for path in [
        "src/arrow_table.rs",
        "src/sinks/mod.rs",
        "src/sinks/arrow/mod.rs",
        "src/sinks/arrow/table.rs",
        "src/sinks/arrow/native_hook.rs",
        "src/sinks/arrow/ftrace.rs",
        "src/sinks/arrow/fixed_result.rs",
        "src/relational/arrow_batch.rs",
        "src/domains/mod.rs",
        "src/domains/fixed_result/mod.rs",
        "src/domains/ftrace/mod.rs",
        "src/domains/native_hook/mod.rs",
        "src/decode/profiler/fixed_result/common.rs",
        "src/decode/profiler/fixed_result/cpu.rs",
        "src/decode/profiler/fixed_result/memory.rs",
        "src/decode/profiler/fixed_result/process.rs",
        "src/decode/profiler/fixed_result/diskio.rs",
        "src/decode/profiler/fixed_result/network.rs",
        "src/decode/profiler/fixed_result/gpu.rs",
        "build/fixed_result_arrow_codegen.rs",
        "build/fixed_result_domain_codegen.rs",
        "build/fixed_result_decoder_codegen.rs",
        "build/ftrace_arrow_codegen.rs",
        "build/native_hook_arrow_codegen.rs",
        "build/native_hook_domain_codegen.rs",
        "build/proto_codegen.rs",
    ] {
        assert!(
            !path_exists(path),
            "{path} belongs to an old generated record/table path and should be removed"
        );
    }
}

#[test]
fn build_script_generates_decoders_and_relational_descriptor_only() {
    let build_rs = source("build.rs");
    let datasource_manifest = source("Cargo.toml");

    for marker in [
        "generate_relational_descriptors",
        "NATIVE_HOOK_PROTO_FILES",
        "FIXED_RESULT_PROTO_FILES",
        ".load_fds(&proto_files, &[\"proto\"])",
        ".compile_fds(fds)",
        ".kat.hitrace",
        ".kat.native_hook",
        "for package in FIXED_RESULT_PROTO_PACKAGES",
        "config.type_attribute(&package",
        "config.enum_attribute(&package",
        ".kat.native_hook.NativeHookData.event",
        "add_sparse_serde_field_attributes",
        "SERDE_SKIP_OPTION_NONE",
        "SERDE_SKIP_VEC_EMPTY",
    ] {
        assert!(build_rs.contains(marker), "{marker} should be wired");
    }
    assert!(
        !datasource_manifest.contains("prost-reflect"),
        "plugin payload decode should use typed prost messages, not prost-reflect"
    );

    assert!(
        build_rs.matches("config.type_attribute").count() >= 2,
        "build.rs should use prost-build type_attribute for generated message serde derives"
    );
    assert!(
        build_rs.contains("config.enum_attribute"),
        "build.rs should use prost-build enum_attribute for generated enum attributes"
    );

    for marker in [
        "fixed_result_domain_codegen",
        "fixed_result_serializable_message_paths",
        "fixed_result_enum_paths",
        "messages_in_file(&fds, proto_path)",
        "native_hook_domain_codegen",
        "native_hook_events_from_descriptor",
        "native_hook_serializable_messages",
        "generate_fixed_result_table_builders",
        "generate_ftrace_event_table_builders",
        "generate_native_hook_table_builders",
        "fixed_result_arrow_codegen",
        "ftrace_arrow_codegen",
        "native_hook_arrow_codegen",
        "fixed_result_decoder_codegen",
        "generate_fixed_result_decoders",
        "fixed_result_table_builders.rs",
        "ftrace_event_table_builders.rs",
        "native_hook_table_builders.rs",
        "generate_protobuf_descriptor_set",
        "PROTOBUF_DESCRIPTOR_SET_FILE",
        "collect_map_entry_messages",
        "collect_map_entry_message",
        "is_map_entry_field",
        "map_entry",
    ] {
        assert!(
            !build_rs.contains(marker),
            "{marker} should not remain in the new relational path"
        );
    }
}

#[test]
fn profiler_decoders_share_typed_plugin_route() {
    let hitrace_mod = source("src/formats/hitrace/mod.rs");
    let registry = source("src/formats/hitrace/profiler/registry.rs");
    let profiler_mod = source("src/decode/profiler/mod.rs");
    let fixed_result_mod = source("src/decode/profiler/fixed_result/mod.rs");
    let ftrace_mod = source("src/decode/profiler/ftrace/mod.rs");
    let native_hook_mod = source("src/decode/profiler/native_hook/mod.rs");
    let decoder_sources =
        format!("{profiler_mod}\n{fixed_result_mod}\n{ftrace_mod}\n{native_hook_mod}");

    for marker in [
        "pub(crate) struct ProfilerPayloadRoute",
        "pub(crate) struct ProfilerPluginRoute",
        "config: Option<ProfilerPayloadRoute>",
        "struct ProfilerPluginDecoder",
        "impl PluginDecoder for ProfilerPluginDecoder",
        "pub(crate) fn new_profiler_plugin_decoder",
        "pub(crate) const PROFILER_PLUGIN_ROUTES: &[ProfilerPluginRoute]",
        "pub(crate) fn profiler_plugin_decoders()",
        "pub(crate) fn emit_typed_payload<T>",
    ] {
        assert!(
            profiler_mod.contains(marker),
            "{marker} should be part of the shared typed profiler decoder route"
        );
    }

    for marker in [
        "const CPU_ROUTE: ProfilerPluginRoute",
        "config: Some(ProfilerPayloadRoute",
        "data: ProfilerPayloadRoute",
        "emit: emit_typed_payload::<CpuConfig>",
        "emit: emit_typed_payload::<CpuData>",
        "emit: emit_typed_payload::<MemoryConfig>",
        "emit: emit_typed_payload::<MemoryData>",
        "emit: emit_typed_payload::<ProcessConfig>",
        "emit: emit_typed_payload::<ProcessData>",
        "emit: emit_typed_payload::<DiskioConfig>",
        "emit: emit_typed_payload::<DiskioData>",
        "emit: emit_typed_payload::<NetworkConfig>",
        "emit: emit_typed_payload::<NetworkDatas>",
        "emit: emit_typed_payload::<GpuConfig>",
        "emit: emit_typed_payload::<GpuData>",
        "const CPU_PLUGIN_NAME",
        "const MEMORY_PLUGIN_NAME",
        "const PROCESS_PLUGIN_NAME",
        "const DISKIO_PLUGIN_NAME",
        "const NETWORK_PLUGIN_NAME",
        "const GPU_PLUGIN_NAME",
    ] {
        assert!(
            fixed_result_mod.contains(marker),
            "{marker} should be part of the fixed result typed route declarations"
        );
    }

    for marker in [
        "const FTRACE_ROUTE: ProfilerPluginRoute",
        "plugin_name: FTRACE_PLUGIN_NAME",
        "config: None",
        "root_message: \"TracePluginResult\"",
        "emit: emit_typed_payload::<TracePluginResult>",
    ] {
        assert!(
            ftrace_mod.contains(marker),
            "{marker} should be part of the ftrace typed route declaration"
        );
    }

    for marker in [
        "const NATIVE_HOOK_ROUTE: ProfilerPluginRoute",
        "const HOOK_DAEMON_ROUTE: ProfilerPluginRoute",
        "config: Some(ProfilerPayloadRoute",
        "root_message: \"NativeHookConfig\"",
        "emit: emit_typed_payload::<NativeHookConfig>",
        "root_message: \"BatchNativeHookData\"",
        "emit: emit_typed_payload::<BatchNativeHookData>",
    ] {
        assert!(
            native_hook_mod.contains(marker),
            "{marker} should be part of the native hook typed route declarations"
        );
    }

    for marker in [
        "fixed_result::CPU_ROUTE",
        "fixed_result::MEMORY_ROUTE",
        "fixed_result::PROCESS_ROUTE",
        "fixed_result::DISKIO_ROUTE",
        "fixed_result::NETWORK_ROUTE",
        "fixed_result::GPU_ROUTE",
        "ftrace::FTRACE_ROUTE",
        "native_hook::NATIVE_HOOK_ROUTE",
        "native_hook::HOOK_DAEMON_ROUTE",
    ] {
        assert!(
            profiler_mod.contains(marker),
            "{marker} should be listed in PROFILER_PLUGIN_ROUTES"
        );
    }
    assert!(hitrace_mod.contains("profiler_plugin_decoders"));
    assert!(hitrace_mod.contains("PluginPayloadRegistry::new(profiler_plugin_decoders())"));
    assert!(!registry.contains("PluginDecoderSpec"));
    assert!(!hitrace_mod.contains("FIXED_RESULT_PLUGIN_DECODERS"));
    assert!(!hitrace_mod.contains("FTRACE_PLUGIN_DECODER"));
    assert!(!hitrace_mod.contains("NATIVE_HOOK_PLUGIN_DECODER"));
    assert!(!hitrace_mod.contains("HOOK_DAEMON_PLUGIN_DECODER"));

    assert!(
        !fixed_result_mod.contains("include!(concat!(env!(\"OUT_DIR\")"),
        "fixed result decoders should not be build-generated"
    );
    assert!(
        !fixed_result_mod.contains("macro_rules! fixed_result_plugins"),
        "fixed result decoder should use explicit constructors instead of a macro registry"
    );
    for marker in [
        "FixedResultPluginDecoder<C, D>",
        "PhantomData",
        "fn dynamic_value_to_json",
        "fn dynamic_map_to_json",
        "fn map_key_to_string",
        "message.descriptor().fields()",
        "message.get_field",
        "dynamic_message_to_json",
        "SerializeOptions",
        "serde_json",
        "prost_reflect",
        "DescriptorPool",
        "DynamicMessage",
        "struct FixedResultRoute",
        "struct FixedResultPluginDecoder",
        "struct FtracePluginDecoder",
        "struct NativeHookPluginDecoder",
        "PluginDecoderSpec",
        "FIXED_RESULT_PLUGIN_DECODERS",
        "FTRACE_PLUGIN_DECODER",
        "NATIVE_HOOK_PLUGIN_DECODER",
        "HOOK_DAEMON_PLUGIN_DECODER",
        "CPU_PLUGIN_DECODER",
        "MEMORY_PLUGIN_DECODER",
        "PROCESS_PLUGIN_DECODER",
        "DISKIO_PLUGIN_DECODER",
        "NETWORK_PLUGIN_DECODER",
        "GPU_PLUGIN_DECODER",
        "fn new_cpu_plugin_decoder",
        "fn new_memory_plugin_decoder",
        "fn new_process_plugin_decoder",
        "fn new_diskio_plugin_decoder",
        "fn new_network_plugin_decoder",
        "fn new_gpu_plugin_decoder",
        "fn new_ftrace_plugin_decoder",
        "fn new_native_hook_plugin_decoder",
        "fn new_hook_daemon_plugin_decoder",
        "fixed_result_message_name",
        "fixed_result_package_name",
        "data_message_name",
        "upper_camel_plugin_stem",
        "match self.route.plugin_name",
        "unsupported fixed result",
        "emit_fixed_result_payload",
    ] {
        assert!(
            !decoder_sources.contains(marker),
            "{marker} should not remain in shared typed profiler decoder routing"
        );
    }
}

#[test]
fn trace_record_stream_uses_generic_decoded_payload() {
    let record = source("src/record.rs");

    assert!(record.contains("pub(crate) struct DecodedPayload"));
    assert!(record.contains("pub(crate) message: PayloadValue"));
    assert!(record.contains("DecodedPayload(Box<DecodedPayload>)"));
    assert!(record.contains("from_typed_message"));

    for marker in [
        "Ftrace(Box<FtraceRecord>)",
        "NativeHook(Box<NativeHookRecord>)",
        "NativeHookBatch",
        "FixedResult(Box<FixedResultRecord>)",
        "NativeHookRecord",
    ] {
        assert!(
            !record.contains(marker),
            "{marker} should not remain as a TraceRecord variant"
        );
    }
}

#[test]
fn profiler_decoders_emit_generic_decoded_payloads_not_table_records() {
    let decoder_sources = joined_sources(&[
        "src/decode/profiler/mod.rs",
        "src/decode/profiler/fixed_result/mod.rs",
        "src/decode/profiler/ftrace/mod.rs",
        "src/decode/profiler/native_hook/mod.rs",
    ]);

    assert!(decoder_sources.contains("TraceRecord::DecodedPayload"));
    assert!(decoder_sources.contains("DecodedPayload::from_typed_message"));
    assert!(decoder_sources.contains("emit_typed_payload::<TracePluginResult>"));
    assert!(decoder_sources.contains("emit_typed_payload::<NativeHookConfig>"));
    assert!(decoder_sources.contains("emit_typed_payload::<BatchNativeHookData>"));

    for marker in [
        "TraceRecord::Ftrace",
        "TraceRecord::NativeHook",
        "TraceRecord::NativeHookBatch",
        "TraceRecord::FixedResult",
        "NativeHookRecord",
        "FixedResultRecord",
        "ContextColumn",
        "context_columns",
        "for detail in result.ftrace_cpu_detail",
        "for event in detail.event",
    ] {
        assert!(
            !decoder_sources.contains(marker),
            "{marker} should not be emitted by profiler decoders"
        );
    }
}

#[test]
fn hitrace_dataset_materializer_uses_relational_sink_not_arrow_table_set() {
    let materializer = source("src/materializer.rs");

    assert!(materializer.contains("RelationalDatasetSink::new"));

    for marker in [
        "ArrowTableSet",
        "ArrowSink",
        "HitraceDatasetSink",
        "HITRACE_DATASET_FLUSH_RECORDS",
        "write_hitrace_tables",
    ] {
        assert!(
            !materializer.contains(marker),
            "{marker} should not remain in .htrace dataset materialize"
        );
    }
}

#[test]
fn dataset_writer_stages_tables_next_to_target_and_validates_before_promote() {
    let writer = source("src/dataset/writer.rs");

    for marker in [
        ".tempdir_in(parent)",
        "failed to validate temporary dataset",
        "register_dataset_tables(&ctx, self.temp_dir.path())",
    ] {
        assert!(
            writer.contains(marker),
            "{marker} should be part of dataset staging and validation"
        );
    }

    for marker in [
        ".tempdir()",
        "copy_dataset_dir",
        "copy_dataset_dir_entries",
        "remove_partial_target",
        "ErrorKind::CrossesDevices",
    ] {
        assert!(
            !writer.contains(marker),
            "{marker} should not remain as dataset staging/promotion behavior"
        );
    }

    for marker in ["/mnt/d", "drvfs", "Windows", "ftrace", "TracePluginResult"] {
        assert!(
            !writer.contains(marker),
            "{marker} should not appear in generic dataset writer staging"
        );
    }
}

#[test]
fn relational_sink_uses_table_buffers_and_streaming_flush() {
    let sink = source("src/relational/sink.rs");
    let table_batch = source("src/relational/table_batch.rs");

    for marker in [
        "const RELATIONAL_TABLE_BUFFER_MAX_ROWS",
        "const RELATIONAL_TABLE_BUFFER_MAX_ESTIMATED_BYTES",
        "struct TableBuffer",
        "struct TableColumnBuilders",
        "enum ColumnBuilder",
        "estimated_bytes",
        "next_row_index",
        "buffered_rows",
        "append_row",
        "finish_record_batch",
        "fn should_flush",
        "fn flush_table",
        "estimate_cell_bytes",
    ] {
        assert!(
            table_batch.contains(marker),
            "{marker} should be part of relational table batch buffering"
        );
    }

    for marker in [
        "fn flush_all_tables",
        "fn flush_pending_payloads",
        "struct PayloadChunk",
        "self.parent_indexes.clear()",
    ] {
        assert!(
            sink.contains(marker),
            "{marker} should be part of streaming relational sink"
        );
    }

    for marker in [
        "struct TableBuffer",
        "struct TableColumnBuilders",
        "enum ColumnBuilder",
        "finish_record_batch",
        "estimate_cell_bytes",
    ] {
        assert!(
            !sink.contains(marker),
            "{marker} should live in relational table batch buffering"
        );
    }

    for marker in [
        "struct TableRows",
        "for (table_name, table) in self.tables",
        "let row_index = table.rows.len() as u64",
        "rows: Vec<RelationalRow>",
        "build_record_batch(",
        "values_for_source(source.value",
        "let row_sources = row_sources_at_path",
        "columns.clone()",
    ] {
        assert!(
            !sink.contains(marker),
            "{marker} should not remain in streaming relational sink"
        );
    }
}

#[test]
fn relational_sink_compiles_plan_and_dispatches_present_items_generically() {
    let sink = source("src/relational/sink.rs");
    let plan_exec = source("src/relational/plan_exec.rs");
    let table_data = source("src/relational/table_data.rs");

    for marker in [
        "struct CompiledRootPlan",
        "struct CompiledPlanItem",
        "struct DispatchStep",
        "struct OptionalChildDispatch",
        "compiled_plans",
        "items_for_payload",
        "optional_child_field",
        "compile_dispatch_steps",
        "optional_child_groups",
        "collect_present_child_fields_at_path",
        "parent_table_by_segment",
    ] {
        assert!(
            plan_exec.contains(marker),
            "{marker} should exist in relational plan execution"
        );
    }

    for marker in [
        "parent_index_for_table",
        "visit_row_sources_at_path",
        "append_table_values",
        "append_value_row_values",
        "leaf_field_descriptor",
        "table_columns",
        "struct RowSource",
        "struct MessageValuePlan",
        "type Ordinals = SmallVec<[usize; 4]>",
    ] {
        assert!(
            table_data.contains(marker),
            "{marker} should exist in relational table data extraction"
        );
    }

    for marker in [
        "struct CompiledRootPlan",
        "struct CompiledPlanItem",
        "struct DispatchStep",
        "struct OptionalChildDispatch",
        "items_for_payload",
        "collect_present_child_fields_at_path",
        "parent_index_for_table",
        "visit_row_sources_at_path",
        "append_table_values",
        "append_value_row_values",
        "struct RowSource",
        "struct MessageValuePlan",
        "type Ordinals = SmallVec<[usize; 4]>",
    ] {
        assert!(
            !sink.contains(marker),
            "{marker} should not remain in relational sink entrypoint"
        );
    }

    for marker in ["compiled_plans", "HashMap<String, HashMap<Ordinals, u64>>"] {
        assert!(
            sink.contains(marker),
            "{marker} should exist for fast generic relational dispatch"
        );
    }

    assert!(
        !sink.contains(".filter(|item| item.root_message == payload.root_message)"),
        "payload emit should not scan every root plan item on every payload"
    );
    for marker in [
        "collect_present_ftrace_event_fields",
        "ftrace_event_field",
        "TracePluginResult",
        "ftrace_cpu_detail",
        "sched_",
    ] {
        assert!(
            !sink.contains(marker),
            "{marker} should not be part of generic relational dispatch"
        );
    }
}

#[test]
fn relational_plan_indexes_only_tables_referenced_as_parents() {
    let plan_exec = source("src/relational/plan_exec.rs");

    for marker in [
        "needs_parent_index",
        "parent_index_tables",
        "parent_table_by_segment",
    ] {
        assert!(
            plan_exec.contains(marker),
            "{marker} should derive internal parent indexes from the compiled root plan"
        );
    }
    for marker in ["FtraceEvent", "ftrace_cpu_detail", "sched_"] {
        assert!(
            !plan_exec.contains(marker),
            "{marker} should not decide which relational tables need parent indexes"
        );
    }
}

#[test]
fn relational_payload_child_avoids_fallback_key_allocation_on_direct_hit() {
    let table_data = source("src/relational/table_data.rs");

    for marker in [
        "fn json_child",
        "if let Some(value) = payload_child(value, field_name)",
        "return Some(value);",
        "let upper_camel = snake_to_upper_camel(field_name);",
    ] {
        assert!(
            table_data.contains(marker),
            "{marker} should keep hot-path payload field lookup allocation-light"
        );
    }
}

#[test]
fn relational_table_value_append_uses_compiled_message_value_plan() {
    let table_data = source("src/relational/table_data.rs");

    for marker in [
        "struct MessageValuePlan",
        "struct ScalarValueField",
        "struct OneofGroupValuePlan",
        "static MESSAGE_VALUE_PLANS",
        "fn message_value_plan",
        "scalar_fields",
        "oneof_groups",
        "field_by_json_key",
        "for scalar_field in &plan.scalar_fields",
        "for group in &plan.oneof_groups",
    ] {
        assert!(
            table_data.contains(marker),
            "{marker} should keep per-row table value append descriptor-light"
        );
    }
}

#[test]
fn relational_table_value_append_writes_payload_directly_to_arrow_builders() {
    let table_data = source("src/relational/table_data.rs");
    let table_batch = source("src/relational/table_batch.rs");

    for marker in ["fn append_payload_cell", "fn append_payload_bytes"] {
        assert!(
            table_batch.contains(marker),
            "{marker} should support direct payload-to-Arrow value append"
        );
    }

    for marker in ["builders.append_payload_value"] {
        assert!(
            table_data.contains(marker),
            "{marker} should support direct payload-to-Arrow value append"
        );
    }

    for marker in [
        "let cell = json_to_cell(field_value, &scalar_field.column_type)",
        "builders.append_cell(*column_index, field.name, cell)",
        "let cell = json_to_cell(value, &scalar_type_to_column_type(scalar_type)?)",
        "builders.append_cell(column_index, \"value\", cell)",
    ] {
        assert!(
            !table_data.contains(marker),
            "{marker} should not remain on the per-row scalar append hot path"
        );
    }
}

#[test]
fn relational_sink_has_single_thread_chunk_boundary() {
    let sink = source("src/relational/sink.rs");
    let table_batch = source("src/relational/table_batch.rs");

    for marker in [
        "struct PayloadChunk",
        "RELATIONAL_PAYLOAD_CHUNK_MAX_RECORDS",
        "pending_payloads",
        "flush_pending_payloads",
        "execute_payload_chunk",
    ] {
        assert!(
            sink.contains(marker),
            "{marker} should exist for the single-thread relational payload chunk boundary"
        );
    }

    for marker in ["struct TableChunk", "write_table_chunk"] {
        assert!(
            table_batch.contains(marker),
            "{marker} should exist for the single-thread relational table chunk boundary"
        );
    }

    for marker in ["std::thread::spawn", "spawn_row", "row_task"] {
        assert!(
            !sink.contains(marker),
            "{marker} should not be part of the single-thread chunk boundary"
        );
    }

    for marker in [
        "next_source_index",
        "next_record_index",
        "start_record_index",
        "payload_sequence",
        "record_index",
    ] {
        assert!(
            !sink.contains(marker),
            "{marker} should not remain as an internal relational sequencing field"
        );
    }
}

#[test]
fn relational_parquet_write_uses_one_bounded_fifo_worker() {
    let sink = source("src/relational/sink.rs");
    let table_batch = source("src/relational/table_batch.rs");

    for marker in [
        "struct ParquetWriteWorker",
        "RELATIONAL_PARQUET_WRITE_QUEUE_CAPACITY: usize = 0",
        "sync_channel(RELATIONAL_PARQUET_WRITE_QUEUE_CAPACITY)",
        ".send(chunk)",
        "write_table_chunks",
    ] {
        assert!(
            table_batch.contains(marker),
            "{marker} should define the bounded FIFO Parquet writer"
        );
    }
    assert!(
        sink.contains("ParquetWriteWorker"),
        "the relational sink should hand completed batches to the writer"
    );
    for marker in ["rayon", "par_iter", "writer_workers"] {
        assert!(
            !format!("{sink}\n{table_batch}").contains(marker),
            "{marker} should not introduce multiple or unordered writers"
        );
    }
}

#[test]
fn relational_queries_do_not_use_record_index_as_join_key() {
    for path in [
        "tests/dataset_contract.rs",
        "tests/hitrace_datasource_query.rs",
    ] {
        let source = source(path);
        assert!(
            !source.contains(".record_index ="),
            "{path} should join nested relational tables with source_index + parent_index, not record_index"
        );
    }
}

#[test]
fn relational_sdd_matches_current_public_table_contract() {
    let sdd = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../../docs/superpowers/specs/2026-07-03-datasource-relationalization-prototype-sdd.md",
    ))
    .expect("relational SDD can be read");

    for marker in [
        "source_id",
        "ContextInjectionRule",
        "context injection",
        "ftrace context",
        "父上下文",
    ] {
        assert!(
            !sdd.contains(marker),
            "{marker} should not remain in the relational SDD"
        );
    }

    for marker in [
        "source_index",
        "输入 trace 文件序号",
        "MemoryData -> `memory_data`",
        "batch_native_hook_data__events__alloc_event",
    ] {
        assert!(
            sdd.contains(marker),
            "{marker} should be documented in the relational SDD"
        );
    }
}

#[test]
fn relational_plan_has_no_prototype_message_allowlist_or_fixed_plan() {
    let descriptor_codegen = source("build/relational_descriptor_codegen.rs");
    let plan = source("src/relational/plan.rs");
    let rules = source("src/relational/rules.rs");
    let sink = source("src/relational/sink.rs");
    let descriptor = source("src/relational/descriptor.rs");
    let lib = source("src/lib.rs");
    let relational_sources = format!("{descriptor_codegen}\n{descriptor}\n{plan}\n{rules}\n{sink}");

    assert!(!descriptor_codegen.contains("PROTOTYPE_MESSAGES"));
    assert!(!plan.contains("prototype_expansion_plan"));
    assert!(!lib.contains("prototype_expansion_plan_table_names"));
    assert!(plan.contains("ExpansionRule::MessageFieldTable"));
    assert!(!rules.contains("EventFieldRule"));
    assert!(!rules.contains("FTRACE_EVENT_FIELD_RULE"));
    for marker in [
        "ContextColumn",
        "ContextInjectionRule",
        "FTRACE_CONTEXT_INJECTION_RULE",
        "context_columns",
        "context_values",
        "InheritedColumn",
        "inherited_columns",
        "inherited_values",
    ] {
        assert!(
            !format!("{plan}\n{rules}").contains(marker),
            "{marker} should not remain in the relational planner"
        );
    }

    for marker in [
        "add_root_scalars(&mut items, \"MemoryData\")",
        "add_repeated_message(&mut items, \"MemoryData\"",
        "add_ftrace_event_field(&mut items, \"sched_switch_format\")",
        "add_event_field_tables",
        "EventFieldTable",
        "\"BatchNativeHookData\",",
        "\"FtraceEvent\"",
        "\"event_cpu\"",
        "\"FtraceCpuDetailMsg\"",
        "ftrace_inherited_columns",
        "table_name(\"FtraceEvent\"",
        "MapField",
        "map_entry",
        "is_map_entry",
        "emit_map_field",
        "append_map_values",
        "map_columns",
    ] {
        assert!(
            !relational_sources.contains(marker),
            "{marker} should not remain without real map proto coverage"
        );
    }
}
