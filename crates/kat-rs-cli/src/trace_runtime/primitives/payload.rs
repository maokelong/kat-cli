use anyhow::{Result, bail};

use crate::trace_runtime::adapter::DatasetAdapter;

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
            let escaped_key = key.replace('\'', "''");
            let key_pos = format!("instr({payload}, '{escaped_key}=')");
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
            "{} = '{}'",
            quote_identifier(&marker.column)?,
            marker.equals.replace('\'', "''")
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

fn quote_identifier(identifier: &str) -> Result<String> {
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        bail!("unsafe sqlite identifier: {identifier}");
    }
    Ok(format!("\"{identifier}\""))
}
