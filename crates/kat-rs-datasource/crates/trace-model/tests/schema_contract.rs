use trace_model::{is_trace_table, schema_for_table, schema_manifest, table_names};

#[test]
fn exposes_only_verified_protobuf_htrace_tables() {
    let manifest = schema_manifest();
    let table_names = table_names();
    let manifest_table_names = manifest
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(manifest.tables.len(), 19);
    assert_eq!(table_names, manifest_table_names);

    for table_name in table_names {
        assert!(
            is_trace_table(table_name),
            "table name should be exposed by manifest: {table_name}"
        );
        assert!(
            schema_for_table(table_name).is_some(),
            "missing schema for {table_name}"
        );
    }
}

#[test]
fn arrow_schema_matches_json_manifest() {
    for table in &schema_manifest().tables {
        let schema = schema_for_table(&table.name).expect("manifest table should have schema");
        assert_eq!(
            schema.fields().len(),
            table.columns.len(),
            "field count mismatch for {}",
            table.name
        );

        for (field, column) in schema.fields().iter().zip(&table.columns) {
            assert_eq!(
                field.name(),
                &column.name,
                "field name mismatch for {}",
                table.name
            );
            assert_eq!(
                field.data_type().to_string(),
                column.data_type.as_manifest_str(),
                "field type mismatch for {}.{}",
                table.name,
                column.name
            );
            assert_eq!(
                field.is_nullable(),
                column.nullable,
                "nullable mismatch for {}.{}",
                table.name,
                column.name
            );
        }
    }
}

#[test]
fn rejects_unmapped_tables() {
    for table_name in [
        "symbols",
        "cpu_usage",
        "diskio",
        "sys_mem_measure",
        "js_heap_nodes",
        "bytrace",
        "ftrace_text",
    ] {
        assert!(
            !is_trace_table(table_name),
            "unexpected table name for unmapped table {table_name}"
        );
        assert!(
            schema_for_table(table_name).is_none(),
            "unexpected schema for unmapped table {table_name}"
        );
    }
}
