use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct ExecutionSnapshot {
    pub entry: LoadedResource,
    pub resources: BTreeMap<String, LoadedResource>,
}

#[derive(Clone, Debug)]
pub struct LoadedResource {
    pub coord: String,
    pub path: String,
    pub digest: String,
    pub resource: Resource,
}

#[derive(Clone, Debug)]
pub enum Resource {
    Flow(FlowResource),
    Query(QueryResource),
    Summaries(SummariesResource),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowResource {
    pub kind: ResourceKind,
    pub description: String,
    pub inputs: InputSpec,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputKind>,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
    #[serde(default)]
    pub examples: Vec<serde_yaml_ng::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryResource {
    pub kind: ResourceKind,
    pub description: String,
    pub inputs: InputSpec,
    pub outputs: BTreeMap<String, OutputKind>,
    pub sql: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummariesResource {
    pub kind: ResourceKind,
    pub description: String,
    pub inputs: InputSpec,
    pub outputs: BTreeMap<String, OutputKind>,
    pub summary: SummarySpec,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Flow,
    Query,
    Summaries,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    String,
    Integer,
    Number,
    Boolean,
    Table,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Table,
    Evidence,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
    #[serde(default)]
    pub required: BTreeMap<String, ValueKind>,
    #[serde(default)]
    pub optional: BTreeMap<String, ValueKind>,
    #[serde(default)]
    pub defaults: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FlowStep {
    Run(RunStep),
    IfEmpty(IfEmptyStep),
    RepeatUntil(RepeatUntilStep),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStep {
    pub run: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputBinding>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputBinding {
    pub set: Option<String>,
    pub append: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IfEmptyStep {
    pub if_empty: String,
    #[serde(default)]
    pub then: Vec<FlowStep>,
    #[serde(default, rename = "else")]
    pub else_steps: Vec<FlowStep>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatUntilStep {
    pub repeat_until: Vec<RepeatCondition>,
    #[serde(default)]
    pub body: Vec<FlowStep>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum RepeatCondition {
    Empty { empty: String },
    MaxIterations { max_iterations: serde_json::Value },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummarySpec {
    pub evidence: Vec<EvidenceSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSpec {
    pub id: String,
    pub fact: String,
    #[serde(default)]
    pub metrics: BTreeMap<String, MetricSpec>,
    #[serde(default)]
    pub refs: Vec<RefSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricSpec {
    pub table: String,
    pub aggregate: String,
    pub column: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefSpec {
    pub table: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub order_by: Vec<OrderBySpec>,
    pub max_rows: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderBySpec {
    pub column: String,
    pub direction: String,
}
