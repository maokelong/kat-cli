use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::trace_runtime::analysis::graph::spec::{
    GenericGraphRootSpec, GenericGraphWalkLimitsSpec, GenericGraphWalkStepSpec, GraphEvidenceSpec,
    GraphExpandSpec, GraphNodeExpandSpec, GraphOrderBySpec, GraphOutputSpec,
    GraphProviderInputSpec, GraphProviderSpec, GraphSelectSpec, GraphValueSpec,
};
pub use crate::trace_runtime::analysis::graph::{binding::BindingExpr, predicate::PredicateSpec};
pub use crate::trace_runtime::analysis::plan::{
    AnalysisInputSpec, AnalysisStepSpec, EvidenceRenderStepSpec, ReportRenderStepSpec,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum TransformSpec {
    #[serde(rename = "sql.view")]
    SqlView(SqlViewTransformSpec),
    #[serde(rename = "payload.extract_fields")]
    PayloadExtractFields(PayloadExtractFieldsTransformSpec),
    #[serde(rename = "rules.classify")]
    RulesClassify(RulesClassifyTransformSpec),
    #[serde(rename = "marker.extract_bracket_fields")]
    MarkerExtractBracketFields(MarkerExtractBracketFieldsTransformSpec),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlViewTransformSpec {
    pub id: String,
    #[serde(default)]
    pub inputs: InputTables,
    pub sql: PathBuf,
    pub output: TransformOutputSpec,
    #[serde(default)]
    pub materialize: Option<String>,
    #[serde(default)]
    pub safety: TransformSafetySpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadExtractFieldsTransformSpec {
    pub id: String,
    #[serde(default)]
    pub inputs: InputTables,
    pub output: TransformOutputSpec,
    #[serde(default)]
    pub materialize: Option<String>,
    #[serde(default)]
    pub safety: TransformSafetySpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesClassifyTransformSpec {
    pub id: String,
    #[serde(default)]
    pub inputs: InputTables,
    pub output: TransformOutputSpec,
    #[serde(default)]
    pub materialize: Option<String>,
    #[serde(default)]
    pub safety: TransformSafetySpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerExtractBracketFieldsTransformSpec {
    pub id: String,
    #[serde(default)]
    pub inputs: InputTables,
    pub source: MarkerSourceSpec,
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
    pub output: TransformOutputSpec,
    #[serde(default)]
    pub materialize: Option<String>,
    #[serde(default)]
    pub safety: TransformSafetySpec,
}

impl TransformSpec {
    pub fn id(&self) -> &str {
        match self {
            Self::SqlView(spec) => &spec.id,
            Self::PayloadExtractFields(spec) => &spec.id,
            Self::RulesClassify(spec) => &spec.id,
            Self::MarkerExtractBracketFields(spec) => &spec.id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::SqlView(_) => "sql.view",
            Self::PayloadExtractFields(_) => "payload.extract_fields",
            Self::RulesClassify(_) => "rules.classify",
            Self::MarkerExtractBracketFields(_) => "marker.extract_bracket_fields",
        }
    }

    pub fn inputs(&self) -> &InputTables {
        match self {
            Self::SqlView(spec) => &spec.inputs,
            Self::PayloadExtractFields(spec) => &spec.inputs,
            Self::RulesClassify(spec) => &spec.inputs,
            Self::MarkerExtractBracketFields(spec) => &spec.inputs,
        }
    }

    pub fn output(&self) -> &TransformOutputSpec {
        match self {
            Self::SqlView(spec) => &spec.output,
            Self::PayloadExtractFields(spec) => &spec.output,
            Self::RulesClassify(spec) => &spec.output,
            Self::MarkerExtractBracketFields(spec) => &spec.output,
        }
    }

    pub fn materialize(&self) -> Option<&str> {
        match self {
            Self::SqlView(spec) => spec.materialize.as_deref(),
            Self::PayloadExtractFields(spec) => spec.materialize.as_deref(),
            Self::RulesClassify(spec) => spec.materialize.as_deref(),
            Self::MarkerExtractBracketFields(spec) => spec.materialize.as_deref(),
        }
    }

    pub fn safety(&self) -> &TransformSafetySpec {
        match self {
            Self::SqlView(spec) => &spec.safety,
            Self::PayloadExtractFields(spec) => &spec.safety,
            Self::RulesClassify(spec) => &spec.safety,
            Self::MarkerExtractBracketFields(spec) => &spec.safety,
        }
    }

    pub fn uses_state_template(&self) -> bool {
        match self {
            Self::SqlView(spec) => string_uses_state(&spec.id),
            Self::PayloadExtractFields(spec) => string_uses_state(&spec.id),
            Self::RulesClassify(spec) => string_uses_state(&spec.id),
            Self::MarkerExtractBracketFields(spec) => {
                string_uses_state(&spec.id)
                    || string_uses_state(&spec.source.table)
                    || string_uses_state(&spec.source.column)
                    || string_uses_state(&spec.source.contains)
                    || spec.fields.values().any(|value| string_uses_state(value))
                    || spec.filters.values().any(value_uses_state)
                    || spec
                        .materialize
                        .as_ref()
                        .is_some_and(|value| string_uses_state(value))
            }
        }
    }
}

fn value_uses_state(value: &Value) -> bool {
    match value {
        Value::String(value) => string_uses_state(value),
        Value::Array(values) => values.iter().any(value_uses_state),
        Value::Object(values) => values.values().any(value_uses_state),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn string_uses_state(value: &str) -> bool {
    value.contains("${state")
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkerSourceSpec {
    pub table: String,
    pub column: String,
    pub contains: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
pub enum InputTables {
    #[default]
    Empty,
    List(Vec<String>),
    Map(BTreeMap<String, String>),
}

impl InputTables {
    pub fn table_names(&self) -> Vec<&str> {
        match self {
            Self::Empty => Vec::new(),
            Self::List(values) => values.iter().map(String::as_str).collect(),
            Self::Map(values) => values.values().map(String::as_str).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformOutputSpec {
    pub table: String,
    pub schema: String,
    #[serde(default)]
    pub semantic: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransformSafetySpec {
    #[serde(default)]
    pub allowed_tables: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisSpec {
    pub id: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, AnalysisInputSpec>,
    #[serde(default)]
    pub steps: Vec<AnalysisStepSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetSpec {
    #[serde(default)]
    pub rules: BTreeMap<String, Value>,
    #[serde(default)]
    pub extractors: BTreeMap<String, Value>,
}
