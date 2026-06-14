use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct ProbeManifest {
    pub id: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputSpec>,
    #[serde(default)]
    pub pipeline: Vec<PipelineStep>,
    pub outputs: OutputSpec,
    #[serde(default)]
    pub safety: SafetySpec,
}

#[derive(Debug, Deserialize)]
pub struct InputSpec {
    #[serde(rename = "type")]
    pub value_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub default: Option<Value>,
}

#[derive(Debug)]
pub enum PipelineStep {
    CreateView(CreateViewStep),
    QueryWindow(QueryWindowStep),
    Operator(OperatorStep),
}

impl<'de> Deserialize<'de> for PipelineStep {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PipelineStepEntry {
            create_view: Option<CreateViewStep>,
            query_window: Option<QueryWindowStep>,
            operator: Option<OperatorStep>,
        }

        let entry = PipelineStepEntry::deserialize(deserializer)?;
        match (entry.create_view, entry.query_window, entry.operator) {
            (Some(step), None, None) => Ok(Self::CreateView(step)),
            (None, Some(step), None) => Ok(Self::QueryWindow(step)),
            (None, None, Some(step)) => Ok(Self::Operator(step)),
            (None, None, None) => Err(D::Error::custom("unknown pipeline step")),
            _ => Err(D::Error::custom(
                "pipeline step must contain exactly one kind",
            )),
        }
    }
}

impl PipelineStep {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::CreateView(_) => "create_view",
            Self::QueryWindow(_) => "query_window",
            Self::Operator(_) => "operator",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateViewStep {
    pub name: String,
    pub sql: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct QueryWindowStep {
    pub target: String,
    #[serde(default)]
    pub mode: QueryMode,
    #[serde(default)]
    pub time_column: Option<String>,
    #[serde(default)]
    pub duration_column: Option<String>,
    #[serde(default)]
    pub filters: BTreeMap<String, String>,
    #[serde(default)]
    pub limit: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMode {
    #[default]
    Window,
    Full,
    Metadata,
}

#[derive(Debug, Deserialize)]
pub struct OperatorStep {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct OutputSpec {
    pub schema: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct SafetySpec {
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub writes: Vec<String>,
    #[serde(default)]
    pub allowed_tables: Vec<String>,
    #[serde(default)]
    pub max_rows: Option<u32>,
}

pub fn registry_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("trace-registry")
}

pub fn load_manifest(root: &Path, probe_id: &str) -> Result<ProbeManifest> {
    let path = root.join(probe_id).join("pipeline.yaml");
    if !path.is_file() {
        bail!("unknown probe `{probe_id}`");
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let manifest: ProbeManifest = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(manifest)
}
