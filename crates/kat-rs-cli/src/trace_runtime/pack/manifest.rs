use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::spec::{RuleSetSpec, TransformSpec};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub schemas: Vec<PathBuf>,
    #[serde(default)]
    pub derived: Vec<PathBuf>,
    #[serde(default)]
    pub queries: Vec<PathBuf>,
    #[serde(default)]
    pub rules: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct LoadedPack {
    pub root: PathBuf,
    pub manifest: PackManifest,
    pub transforms: Vec<TransformSpec>,
    pub rule_sets: Vec<RuleSetSpec>,
}

pub fn load_pack(root: impl AsRef<Path>) -> Result<LoadedPack> {
    let root = root.as_ref();
    let manifest_path = root.join("pack.yaml");
    let manifest: PackManifest = read_yaml(&manifest_path)?;
    validate_pack_id(&manifest.id)?;

    let transform_specs = manifest
        .derived
        .iter()
        .map(|path| {
            let spec = read_yaml::<TransformSpec>(&required_file(root, path)?)?;
            Ok((path.clone(), spec))
        })
        .collect::<Result<Vec<_>>>()?;
    reject_duplicate_transform_ids(&transform_specs)?;
    let transforms = transform_specs.into_iter().map(|(_, spec)| spec).collect();

    let rule_sets = manifest
        .rules
        .iter()
        .map(|path| read_yaml::<RuleSetSpec>(&required_file(root, path)?))
        .collect::<Result<Vec<_>>>()?;

    for path in manifest.schemas.iter().chain(manifest.queries.iter()) {
        required_file(root, path)?;
    }

    Ok(LoadedPack {
        root: root.to_path_buf(),
        manifest,
        transforms,
        rule_sets,
    })
}

fn read_yaml<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn required_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_pack_reference(relative)?;
    let path = root.join(relative);
    if !path.is_file() {
        bail!("pack referenced file is missing: {}", relative.display());
    }
    Ok(path)
}

fn validate_pack_reference(relative: &Path) -> Result<()> {
    for component in relative.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                bail!(
                    "pack referenced file escapes pack root: {}",
                    relative.display()
                );
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

fn validate_pack_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        bail!("invalid pack id: {id:?}");
    }
    Ok(())
}

fn reject_duplicate_transform_ids(transforms: &[(PathBuf, TransformSpec)]) -> Result<()> {
    let mut seen = BTreeMap::new();
    for (path, transform) in transforms {
        if let Some(first_path) = seen.insert(transform.id.as_str(), path.as_path()) {
            bail!(
                "duplicate transform id {}: first declared at {}, duplicate declared at {}",
                transform.id,
                first_path.display(),
                path.display()
            );
        }
    }
    Ok(())
}

