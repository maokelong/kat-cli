use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::trace_runtime::analysis::graph::spec::GenericGraphWalkStepSpec;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisInputSpec {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRequiresSpec {
    #[serde(default)]
    pub derived: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum AnalysisStepSpec {
    #[serde(rename = "evidence.render")]
    EvidenceRender(EvidenceRenderStepSpec),
    #[serde(rename = "graph.walk")]
    GraphWalk(GenericGraphWalkStepSpec),
    #[serde(rename = "report.render")]
    ReportRender(ReportRenderStepSpec),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRenderStepSpec {
    pub id: String,
    pub from: String,
    #[serde(default)]
    pub writes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRenderStepSpec {
    pub id: String,
}
