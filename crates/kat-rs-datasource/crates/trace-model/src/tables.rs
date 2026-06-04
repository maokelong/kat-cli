use crate::schema_manifest;

pub fn table_names() -> Vec<&'static str> {
    schema_manifest()
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect()
}

pub fn is_trace_table(table_name: &str) -> bool {
    schema_manifest().table(table_name).is_some()
}
