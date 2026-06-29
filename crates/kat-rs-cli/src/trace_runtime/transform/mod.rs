use anyhow::Result;
use serde_json::Value;

use crate::trace_runtime::{
    adapter::DatasetAdapter,
    pack::{LoadedPack, spec::TransformSpec},
};

pub mod derived_runner;
pub mod marker;
pub mod payload;
pub mod primitives;
pub mod rules;
pub mod sql;

pub fn run_transform(
    adapter: &mut dyn DatasetAdapter,
    pack: &LoadedPack,
    transform: &TransformSpec,
    params: &Value,
    state: &Value,
) -> Result<()> {
    match transform {
        TransformSpec::SqlView(spec) => {
            sql::run_sql_view_transform(adapter, &pack.root, spec, params)
        }
        TransformSpec::PayloadExtractFields(spec) => {
            payload::run_payload_extract_fields_transform(adapter, pack, spec)
        }
        TransformSpec::RulesClassify(spec) => {
            rules::run_rules_classify_transform(adapter, pack, spec)
        }
        TransformSpec::MarkerExtractBracketFields(spec) => {
            marker::run_marker_extract_bracket_fields_transform(adapter, spec, params, state)
        }
    }
}
