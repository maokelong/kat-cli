use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
    primitives::payload::{PayloadExtractorSpec, PayloadMarkerFilter, run_payload_extract_fields},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadExtractorConfig {
    source_table: String,
    payload_column: String,
    #[serde(default)]
    marker: Option<PayloadMarkerConfig>,
    fields: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadMarkerConfig {
    column: String,
    equals: String,
}

pub fn run_payload_extract_fields_transform(
    adapter: &mut dyn DatasetAdapter,
    pack: &LoadedPack,
    transform: &TransformSpec,
) -> Result<()> {
    if transform.kind != "payload.extract_fields" {
        bail!("transform `{}` is not payload.extract_fields", transform.id);
    }
    for table in transform.inputs.table_names() {
        if !adapter.table_exists(table)? {
            bail!(
                "transform `{}` input table does not exist: {table}",
                transform.id
            );
        }
    }

    let extractor = pack
        .rule_sets
        .iter()
        .find_map(|rule_set| rule_set.extractors.get(&transform.id))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "payload.extract_fields transform `{}` has no matching extractor config",
                transform.id
            )
        })?;
    let config: PayloadExtractorConfig = serde_json::from_value(extractor.clone())
        .with_context(|| format!("failed to parse extractor config for `{}`", transform.id))?;
    let input_tables = transform.inputs.table_names();
    if !input_tables
        .iter()
        .any(|table| *table == config.source_table)
    {
        bail!(
            "payload.extract_fields transform `{}` source_table `{}` is outside transform inputs: {}",
            transform.id,
            config.source_table,
            input_tables.join(", ")
        );
    }
    if transform.safety.allowed_tables.is_empty() {
        bail!(
            "payload.extract_fields transform `{}` requires non-empty safety.allowedTables",
            transform.id
        );
    }
    if !transform
        .safety
        .allowed_tables
        .iter()
        .any(|table| table == &config.source_table)
    {
        bail!(
            "payload.extract_fields transform `{}` source_table `{}` is outside safety.allowedTables: {}",
            transform.id,
            config.source_table,
            transform.safety.allowed_tables.join(", ")
        );
    }
    if !adapter.table_exists(&config.source_table)? {
        bail!(
            "payload.extract_fields transform `{}` source_table does not exist: {}",
            transform.id,
            config.source_table
        );
    }
    if config.fields.is_empty() {
        bail!(
            "payload.extract_fields transform `{}` has no fields",
            transform.id
        );
    }
    let marker = config.marker.map(|marker| PayloadMarkerFilter {
        column: marker.column,
        equals: marker.equals,
    });
    let spec = PayloadExtractorSpec {
        source_table: config.source_table,
        output_table: transform.output.table.clone(),
        payload_column: config.payload_column,
        marker,
        fields: config.fields.into_iter().collect(),
    };

    run_payload_extract_fields(adapter, &spec)
}
