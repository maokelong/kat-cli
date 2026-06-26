use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisInputSpec {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRequiresSpec {
    #[serde(default)]
    pub derived: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum AnalysisStepSpec {
    #[serde(rename = "evidence.render")]
    EvidenceRender(EvidenceRenderStepSpec),
    #[serde(rename = "temporal.graph_walk")]
    TemporalGraphWalk(GraphWalkStepSpec),
    #[serde(rename = "report.render")]
    ReportRender(ReportRenderStepSpec),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRenderStepSpec {
    pub id: String,
    pub from: String,
    #[serde(default)]
    pub writes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphWalkStepSpec {
    pub id: String,
    pub root: GraphWalkRootSpec,
    #[serde(default)]
    pub limits: GraphWalkLimitsSpec,
    #[serde(default)]
    pub edge_providers: Vec<EdgeProviderSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphWalkRootSpec {
    pub from_state: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphWalkLimitsSpec {
    #[serde(default = "default_graph_walk_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_graph_walk_max_edges_per_node")]
    pub max_edges_per_node: usize,
}

impl Default for GraphWalkLimitsSpec {
    fn default() -> Self {
        Self {
            max_depth: default_graph_walk_max_depth(),
            max_edges_per_node: default_graph_walk_max_edges_per_node(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeProviderSpec {
    pub id: String,
    pub table: String,
    #[serde(default)]
    pub when: BTreeMap<String, ConditionOp>,
    pub emit: EdgeEmitSpec,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ConditionOp {
    Eq(Value),
    Neq(Value),
    Gte(f64),
    Gt(f64),
    Lte(f64),
    Lt(f64),
    Exists(bool),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeEmitSpec {
    pub edge_type: String,
    pub target: EdgeTargetSpec,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeTargetSpec {
    #[serde(default, alias = "same_node")]
    pub same_node: bool,
    #[serde(default)]
    pub itid: Option<String>,
    #[serde(default, alias = "start_ts")]
    pub start_ts: Option<String>,
    #[serde(default, alias = "end_ts")]
    pub end_ts: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRenderStepSpec {
    pub id: String,
}

const fn default_graph_walk_max_depth() -> usize {
    3
}

const fn default_graph_walk_max_edges_per_node() -> usize {
    3
}
