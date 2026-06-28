use anyhow::{Result, bail};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
};

pub mod marker;
pub mod payload;
pub mod primitives;
pub mod derived_runner;
pub mod rules;
pub mod sql;

pub fn run_transform(
    adapter: &mut dyn DatasetAdapter,
    pack: &LoadedPack,
    transform: &TransformSpec,
    params: &Value,
    state: &Value,
) -> Result<()> {
    match transform.kind.as_str() {
        "sql.view" => sql::run_sql_view_transform(adapter, &pack.root, transform, params),
        "payload.extract_fields" => {
            payload::run_payload_extract_fields_transform(adapter, pack, transform)
        }
        "rules.classify" => rules::run_rules_classify_transform(adapter, pack, transform),
        "marker.extract_bracket_fields" => {
            marker::run_marker_extract_bracket_fields_transform(adapter, transform, params, state)
        }
        other => bail!(
            "unsupported transform kind `{other}` for `{}`",
            transform.id
        ),
    }
}

pub(crate) fn reject_marker_only_config(transform: &TransformSpec, kind: &str) -> Result<()> {
    if transform.source.is_some()
        || !transform.fields.is_empty()
        || !transform.joins.is_empty()
        || !transform.filters.is_empty()
    {
        bail!(
            "{kind} transform `{}` does not support marker-only config fields",
            transform.id
        );
    }
    Ok(())
}
