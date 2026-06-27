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
    AnalysisInputSpec, AnalysisRequiresSpec, AnalysisStepSpec, ConditionOp, EdgeEmitSpec,
    EdgeFactRowSpec, EdgeFactSpec, EdgeProviderSpec, EdgeTargetSpec, EvidenceRenderStepSpec,
    GraphWalkLimitsSpec, GraphWalkRootSpec, GraphWalkStepSpec, ReportRenderStepSpec,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformSpec {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub inputs: InputTables,
    #[serde(default)]
    pub sql: Option<PathBuf>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub bind: BTreeMap<String, String>,
    #[serde(default, rename = "where")]
    pub where_: BTreeMap<String, Value>,
    #[serde(default)]
    pub source: Option<MarkerSourceSpec>,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub joins: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
    pub output: TransformOutputSpec,
    #[serde(default)]
    pub materialize: Option<String>,
    #[serde(default)]
    pub safety: TransformSafetySpec,
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
    pub requires: AnalysisRequiresSpec,
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
