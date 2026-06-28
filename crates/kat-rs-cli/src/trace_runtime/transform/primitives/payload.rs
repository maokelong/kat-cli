use anyhow::Result;

use crate::trace_runtime::adapter::{
    DatasetAdapter,
    sqlite::sql::{quote_identifier, string_literal},
};

pub struct PayloadExtractorSpec {
    pub source_table: String,
    pub output_table: String,
    pub payload_column: String,
    pub marker: Option<PayloadMarkerFilter>,
    pub fields: Vec<(String, String)>,
}

pub struct PayloadMarkerFilter {
    pub column: String,
    pub equals: String,
}

pub fn run_payload_extract_fields(
    adapter: &mut dyn DatasetAdapter,
    spec: &PayloadExtractorSpec,
) -> Result<()> {
    let payload = quote_identifier(&spec.payload_column)?;
    let projections = spec
        .fields
        .iter()
        .map(|(output, key)| {
            let key_literal = string_literal(&format!("{key}="));
            let key_pos = format!("instr({payload}, {key_literal})");
            let value_start = format!("{key_pos} + {}", key.len() + 1);
            let value_tail = format!("substr({payload}, {value_start})");
            let comma_pos = format!("instr({value_tail}, ',')");
            Ok(format!(
                "CASE WHEN {key_pos} = 0 THEN NULL ELSE CAST(CASE WHEN {comma_pos} > 0 THEN substr({value_tail}, 1, {comma_pos} - 1) ELSE {value_tail} END AS INTEGER) END AS {output}",
                output = quote_identifier(output)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut filters = vec![format!("{payload} IS NOT NULL")];
    if let Some(marker) = &spec.marker {
        filters.push(format!(
            "{} = {}",
            quote_identifier(&marker.column)?,
            string_literal(&marker.equals)
        ));
    }
    let sql = format!(
        "SELECT {} FROM {} WHERE {}",
        projections.join(", "),
        quote_identifier(&spec.source_table)?,
        filters.join(" AND "),
    );
    adapter.create_derived_table_as(&spec.output_table, &sql)
}
