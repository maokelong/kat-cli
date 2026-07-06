use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::ApiError;

#[derive(Clone, Debug)]
pub struct ResourceRoot {
    root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LoadedYaml<T> {
    pub path: PathBuf,
    pub digest: String,
    pub value: T,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub schema_version: u64,
    pub kind: String,
    #[serde(default)]
    pub resources: ManifestResources,
    #[serde(default)]
    pub packs: HashMap<String, ManifestPack>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestPack {
    pub summary: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ManifestResources {
    #[serde(default)]
    pub flows: HashMap<String, ManifestResource>,
    #[serde(default)]
    pub grep: HashMap<String, ManifestResource>,
    #[serde(default)]
    pub query: HashMap<String, ManifestResource>,
    #[serde(default)]
    pub summaries: HashMap<String, ManifestResource>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestResource {
    pub summary: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pack {
    pub pack: PackIdentity,
    #[serde(default)]
    pub inputs: Value,
    #[serde(default)]
    pub requires: Value,
    #[serde(default)]
    pub imports: PackImports,
    pub entry_flow: String,
    #[serde(default)]
    pub outputs: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackIdentity {
    pub id: String,
    pub title: String,
    pub domain: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PackImports {
    #[serde(default)]
    pub flows: HashMap<String, String>,
    #[serde(default)]
    pub summaries: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Flow {
    pub id: String,
    #[serde(default)]
    pub constants: Value,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FlowStep {
    pub id: String,
    pub uses: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrepResource {
    pub id: String,
    #[serde(default)]
    pub context: ResourceContext,
    pub output: TableOutput,
    pub target: GrepTarget,
    #[serde(default)]
    pub patterns: Vec<GrepPattern>,
    #[serde(default)]
    pub predicates: Vec<GrepPredicate>,
    #[serde(default)]
    pub order_by: Vec<OrderBy>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QueryResource {
    pub id: String,
    #[serde(default)]
    pub context: ResourceContext,
    pub output: TableOutput,
    pub sql: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SummaryResource {
    pub id: String,
    pub summary: SummaryBody,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BriefResource {
    #[serde(default)]
    pub sections: Vec<BriefSection>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ResourceContext {
    #[serde(default)]
    pub publishes: HashMap<String, PublishSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PublishSpec {
    pub carrier: String,
    pub from: PublishFrom,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PublishFrom {
    pub column: Option<String>,
    pub start_column: Option<String>,
    pub end_column: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TableOutput {
    pub table: String,
    #[serde(default)]
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrepTarget {
    pub table: String,
    #[serde(default)]
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrepPattern {
    pub value: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrepPredicate {
    pub column: String,
    pub equals: Option<String>,
    #[serde(default)]
    pub is_not_null: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderBy {
    pub column: String,
    pub direction: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SummaryBody {
    #[serde(default)]
    pub evidence: Vec<EvidenceSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EvidenceSpec {
    pub id: String,
    pub fact: String,
    #[serde(default)]
    pub metrics: HashMap<String, MetricSpec>,
    #[serde(default)]
    pub refs: Vec<RefSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MetricSpec {
    pub table: String,
    pub column: Option<String>,
    pub aggregate: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RefSpec {
    pub table: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub order_by: Vec<OrderBy>,
    pub max_rows: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BriefSection {
    pub id: String,
    #[serde(rename = "from")]
    pub from_table: String,
    #[serde(default)]
    pub include: Vec<String>,
    pub order_by: Option<BriefOrderBy>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BriefOrderBy {
    pub field: String,
    pub direction: Option<String>,
}

impl ResourceRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if root.is_relative() && !root.exists() {
            let package_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            for ancestor in package_dir.ancestors() {
                let candidate = ancestor.join(&root);
                if candidate.exists() {
                    return Self { root: candidate };
                }
            }
        }

        Self { root }
    }

    pub fn load_manifest(&self) -> Result<LoadedYaml<Manifest>, ApiError> {
        self.load_yaml("manifest.yaml")
    }

    pub fn load_pack(
        &self,
        manifest: &Manifest,
        pack_ref: &str,
    ) -> Result<LoadedYaml<Pack>, ApiError> {
        let pack = manifest.packs.get(pack_ref).ok_or_else(|| {
            ApiError::validation(format!("pack is not declared in manifest: {pack_ref}"))
        })?;

        self.load_yaml(&pack.path)
    }

    pub fn load_flow_by_path(&self, path: impl AsRef<Path>) -> Result<LoadedYaml<Flow>, ApiError> {
        self.load_yaml(path)
    }

    pub fn load_flow_resource(
        &self,
        manifest: &Manifest,
        resource_ref: &str,
    ) -> Result<LoadedYaml<Flow>, ApiError> {
        let resource = manifest.resources.flows.get(resource_ref).ok_or_else(|| {
            ApiError::validation(format!(
                "flow resource is not declared in manifest: {resource_ref}"
            ))
        })?;

        self.load_yaml(&resource.path)
    }

    pub fn load_grep_resource(
        &self,
        manifest: &Manifest,
        resource_ref: &str,
    ) -> Result<LoadedYaml<GrepResource>, ApiError> {
        let resource = manifest.resources.grep.get(resource_ref).ok_or_else(|| {
            ApiError::validation(format!(
                "grep resource is not declared in manifest: {resource_ref}"
            ))
        })?;

        self.load_yaml(&resource.path)
    }

    pub fn load_query_resource(
        &self,
        manifest: &Manifest,
        resource_ref: &str,
    ) -> Result<LoadedYaml<QueryResource>, ApiError> {
        let resource = manifest.resources.query.get(resource_ref).ok_or_else(|| {
            ApiError::validation(format!(
                "query resource is not declared in manifest: {resource_ref}"
            ))
        })?;

        self.load_yaml(&resource.path)
    }

    pub fn load_summary_resource(
        &self,
        manifest: &Manifest,
        resource_ref: &str,
    ) -> Result<LoadedYaml<SummaryResource>, ApiError> {
        let resource = manifest
            .resources
            .summaries
            .get(resource_ref)
            .ok_or_else(|| {
                ApiError::validation(format!(
                    "summary resource is not declared in manifest: {resource_ref}"
                ))
            })?;

        self.load_yaml(&resource.path)
    }

    pub fn load_entry_flow(&self, pack: &LoadedYaml<Pack>) -> Result<LoadedYaml<Flow>, ApiError> {
        let pack_dir = pack.path.parent().ok_or_else(|| {
            ApiError::validation(format!(
                "pack path does not have parent directory: {}",
                pack.path.display()
            ))
        })?;
        let flow: LoadedYaml<Flow> = self.load_yaml(pack_dir.join("flow.yaml"))?;

        if flow.value.id != pack.value.entry_flow {
            return Err(ApiError::validation(format!(
                "entry flow id {} does not match pack entry_flow {}",
                flow.value.id, pack.value.entry_flow
            )));
        }

        Ok(flow)
    }

    pub fn load_pack_brief(
        &self,
        pack: &LoadedYaml<Pack>,
    ) -> Result<LoadedYaml<BriefResource>, ApiError> {
        let pack_dir = pack.path.parent().ok_or_else(|| {
            ApiError::validation(format!(
                "pack path does not have parent directory: {}",
                pack.path.display()
            ))
        })?;

        self.load_yaml(pack_dir.join("brief.yaml"))
    }

    fn load_yaml<T>(&self, path: impl AsRef<Path>) -> Result<LoadedYaml<T>, ApiError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let path = path.as_ref();
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let bytes = std::fs::read(&full_path).map_err(|error| {
            ApiError::validation(format!(
                "failed to read resource yaml {}: {error}",
                full_path.display()
            ))
        })?;
        let value = serde_yaml::from_slice(&bytes).map_err(|error| {
            ApiError::validation(format!(
                "failed to parse resource yaml {}: {error}",
                full_path.display()
            ))
        })?;
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));

        Ok(LoadedYaml {
            path: full_path,
            digest,
            value,
        })
    }
}
