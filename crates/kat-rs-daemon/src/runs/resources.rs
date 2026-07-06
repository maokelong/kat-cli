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

impl ResourceRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
