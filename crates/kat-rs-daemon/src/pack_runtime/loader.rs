use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::error::ApiError;

use super::model::{
    ExecutionSnapshot, FlowResource, FlowStep, LoadedResource, QueryResource, Resource,
    ResourceKind, SummariesResource,
};

pub fn load_snapshot(pack_root: &Path, pack_ref: &str) -> Result<ExecutionSnapshot, ApiError> {
    validate_pack_ref(pack_ref)?;
    let entry_path = pack_root.join(pack_ref).with_extension("yaml");
    if !entry_path.starts_with(pack_root) {
        return Err(ApiError::validation("packRef escapes pack root"));
    }

    let entry_coord = entry_coord(pack_ref)?;
    let mut resources = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let entry = load_resource(pack_root, &entry_coord, &entry_path)?;
    collect_transitive(pack_root, pack_ref, &entry, &mut visiting, &mut resources)?;
    let entry = resources
        .get(&entry_coord)
        .cloned()
        .ok_or_else(|| ApiError::internal("entry resource missing from snapshot"))?;

    Ok(ExecutionSnapshot { entry, resources })
}

fn collect_transitive(
    pack_root: &Path,
    pack_ref: &str,
    resource: &LoadedResource,
    visiting: &mut BTreeSet<String>,
    resources: &mut BTreeMap<String, LoadedResource>,
) -> Result<(), ApiError> {
    if resources.contains_key(&resource.coord) {
        return Ok(());
    }
    if !visiting.insert(resource.coord.clone()) {
        return Err(ApiError::validation(format!(
            "recursive pack resource reference: {}",
            resource.coord
        )));
    }

    let dependencies = match &resource.resource {
        Resource::Flow(flow) => flow
            .steps
            .iter()
            .flat_map(step_dependencies)
            .collect::<Vec<_>>(),
        Resource::Query(_) | Resource::Summaries(_) => Vec::new(),
    };
    resources.insert(resource.coord.clone(), resource.clone());

    for coord in dependencies {
        let path = coord_to_path(pack_root, pack_ref, &coord)?;
        let child = load_resource(pack_root, &coord, &path)?;
        collect_transitive(pack_root, pack_ref, &child, visiting, resources)?;
    }

    visiting.remove(&resource.coord);
    Ok(())
}

fn step_dependencies(step: &FlowStep) -> Vec<String> {
    match step {
        FlowStep::Run(step) => vec![step.run.clone()],
        FlowStep::IfEmpty(step) => step
            .then
            .iter()
            .chain(step.else_steps.iter())
            .flat_map(step_dependencies)
            .collect(),
        FlowStep::RepeatUntil(step) => step.body.iter().flat_map(step_dependencies).collect(),
    }
}

fn load_resource(pack_root: &Path, coord: &str, path: &Path) -> Result<LoadedResource, ApiError> {
    let content = fs::read_to_string(path).map_err(|error| {
        ApiError::validation(format!(
            "failed to read pack resource {}: {error}",
            path.display()
        ))
    })?;
    let kind = resource_kind(&content)?;
    let resource = match kind {
        ResourceKind::Flow => Resource::Flow(parse_yaml::<FlowResource>(path, &content)?),
        ResourceKind::Query => Resource::Query(parse_yaml::<QueryResource>(path, &content)?),
        ResourceKind::Summaries => {
            Resource::Summaries(parse_yaml::<SummariesResource>(path, &content)?)
        }
    };

    validate_coord_kind(coord, &resource)?;

    let digest = Sha256::digest(content.as_bytes());

    Ok(LoadedResource {
        coord: coord.to_string(),
        path: path
            .strip_prefix(pack_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        digest: format!(
            "sha256:{}",
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        resource,
    })
}

fn parse_yaml<T: serde::de::DeserializeOwned>(path: &Path, content: &str) -> Result<T, ApiError> {
    serde_yaml_ng::from_str(content).map_err(|error| {
        ApiError::validation(format!(
            "failed to parse pack resource {}: {error}",
            path.display()
        ))
    })
}

fn resource_kind(content: &str) -> Result<ResourceKind, ApiError> {
    #[derive(serde::Deserialize)]
    struct KindOnly {
        kind: ResourceKind,
    }

    Ok(serde_yaml_ng::from_str::<KindOnly>(content)
        .map_err(|error| ApiError::validation(format!("failed to read resource kind: {error}")))?
        .kind)
}

fn validate_pack_ref(pack_ref: &str) -> Result<(), ApiError> {
    if pack_ref.is_empty()
        || pack_ref.starts_with('/')
        || pack_ref.starts_with('\\')
        || pack_ref.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || pack_ref.contains('\\')
    {
        return Err(ApiError::validation(format!("invalid packRef: {pack_ref}")));
    }

    Ok(())
}

fn entry_coord(pack_ref: &str) -> Result<String, ApiError> {
    let name = pack_ref
        .rsplit('/')
        .next()
        .ok_or_else(|| ApiError::validation(format!("invalid packRef: {pack_ref}")))?;
    Ok(format!("local.flows.{name}"))
}

fn coord_to_path(pack_root: &Path, pack_ref: &str, coord: &str) -> Result<PathBuf, ApiError> {
    let parts = coord.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(ApiError::validation(format!(
            "invalid resource coordinate: {coord}"
        )));
    }
    let scope = parts[0];
    let kind = parts[1];
    let name = parts[2];
    if !matches!(kind, "flows" | "query" | "summaries") {
        return Err(ApiError::validation(format!(
            "unsupported resource kind in coordinate: {coord}"
        )));
    }

    let path = match scope {
        "common" => pack_root.join("common").join(kind).join(format!("{name}.yaml")),
        "local" => {
            let pack_dir = pack_ref
                .rsplit_once('/')
                .map(|(dir, _)| dir)
                .ok_or_else(|| ApiError::validation(format!("invalid packRef: {pack_ref}")))?;
            pack_root
                .join(pack_dir)
                .join("local")
                .join(kind)
                .join(format!("{name}.yaml"))
        }
        _ => {
            return Err(ApiError::validation(format!(
                "unsupported resource scope in coordinate: {coord}"
            )));
        }
    };

    Ok(path)
}

fn validate_coord_kind(coord: &str, resource: &Resource) -> Result<(), ApiError> {
    let expected = match resource {
        Resource::Flow(_) => ".flows.",
        Resource::Query(_) => ".query.",
        Resource::Summaries(_) => ".summaries.",
    };
    if !coord.contains(expected) {
        return Err(ApiError::validation(format!(
            "resource kind does not match coordinate: {coord}"
        )));
    }

    Ok(())
}
