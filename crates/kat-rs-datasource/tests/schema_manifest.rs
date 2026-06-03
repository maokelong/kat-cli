use kat_rs_datasource::load_schema_manifest;
use std::collections::BTreeSet;
use trace_model::TraceTableBuilder;

fn empty_trace_tables() -> trace_model::TraceTables {
    TraceTableBuilder::default()
        .finish("test-trace".to_string(), None, None, "unknown".to_string())
        .expect("empty trace tables should build")
}

#[test]
fn manifest_table_names_match_trace_tables() {
    let manifest = load_schema_manifest().expect("manifest should load");
    let manifest_names = manifest
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<BTreeSet<_>>();

    let tables = empty_trace_tables();
    let batch_names = tables.batches().keys().copied().collect::<BTreeSet<_>>();

    assert_eq!(manifest_names, batch_names);
}

#[test]
fn manifest_columns_match_arrow_schemas() {
    let manifest = load_schema_manifest().expect("manifest should load");
    let tables = empty_trace_tables();

    for (table_name, batch) in tables.batches() {
        let table = manifest
            .table(table_name)
            .unwrap_or_else(|| panic!("missing manifest table {table_name}"));
        let schema = batch.schema();

        assert_eq!(
            table.columns.len(),
            schema.fields().len(),
            "column count mismatch for {table_name}"
        );

        for (manifest_column, field) in table.columns.iter().zip(schema.fields()) {
            assert_eq!(manifest_column.name, field.name().as_str(), "{table_name}");
            assert_eq!(
                manifest_column.data_type,
                field.data_type().to_string(),
                "{table_name}.{}",
                field.name()
            );
            assert_eq!(
                manifest_column.nullable,
                field.is_nullable(),
                "{table_name}.{}",
                field.name()
            );
        }
    }
}

#[test]
fn manifest_excludes_unimplemented_legacy_tables() {
    let manifest = load_schema_manifest().expect("manifest should load");
    let names = manifest
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<BTreeSet<_>>();

    for stale_name in [
        "log",
        "hisysevent_all_event",
        "hisysevent_measure",
        "perf_report",
        "perf_files",
        "perf_thread",
        "perf_sample",
        "perf_callchain",
    ] {
        assert!(
            !names.contains(stale_name),
            "{stale_name} should not be exposed by the protobuf htrace MVP"
        );
    }
}

#[test]
fn manifest_excludes_unmapped_parser_tables() {
    let manifest = load_schema_manifest().expect("manifest should load");
    let names = manifest
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<BTreeSet<_>>();

    for removed_name in [
        "irq",
        "symbols",
        "dma_fence",
        "cpu_usage",
        "diskio",
        "process_measure",
        "process_measure_filter",
        "sys_mem_measure",
        "sys_event_filter",
        "live_process",
        "js_heap_files",
        "js_heap_info",
        "js_heap_nodes",
        "js_heap_edges",
        "js_heap_string",
        "js_heap_location",
        "js_heap_sample",
        "js_heap_trace_function_info",
        "js_heap_trace_node",
        "js_config",
        "js_cpu_profiler_node",
        "js_cpu_profiler_sample",
    ] {
        assert!(
            !names.contains(removed_name),
            "{removed_name} should not be exposed by the exported trace model"
        );
    }
}
