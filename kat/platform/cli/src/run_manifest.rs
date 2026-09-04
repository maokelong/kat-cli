use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    session_store::{RunId, SessionLayout, metadata_is_reparse_point},
    workflow_runtime::{self, RunOutputMetadata},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunManifest {
    pub(super) session_id: String,
    pub(super) run_id: String,
    pub(super) pack: String,
    pub(super) workflow: String,
    #[serde(
        default,
        rename = "dataset",
        deserialize_with = "deserialize_ignored",
        skip_serializing
    )]
    _legacy_dataset: (),
    pub(super) inputs: BTreeMap<String, serde_json::Value>,
    pub(super) outputs: BTreeMap<String, RunOutputMetadata>,
}

impl RunManifest {
    pub(super) fn new(
        session_id: String,
        run_id: String,
        pack: String,
        workflow: String,
        inputs: BTreeMap<String, serde_json::Value>,
        outputs: BTreeMap<String, RunOutputMetadata>,
    ) -> Self {
        Self {
            session_id,
            run_id,
            pack,
            workflow,
            _legacy_dataset: (),
            inputs,
            outputs,
        }
    }
}

fn deserialize_ignored<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::IgnoredAny::deserialize(deserializer).map(drop)
}

pub(super) struct PublishedRun {
    pub(super) run_id: String,
    pub(super) pack: String,
    pub(super) workflow: String,
    pub(super) outputs: BTreeMap<String, RunOutputMetadata>,
    pub(super) output_paths: BTreeMap<String, String>,
}

pub(super) fn resolve(
    session: &SessionLayout,
    run: &str,
) -> Result<PublishedRun, PublishedRunError> {
    let run_id = RunId::parse(run).ok_or_else(|| PublishedRunError::NotFound {
        session_id: session.session_id().as_str().to_owned(),
        run_id: diagnostic_safe_argument(run),
    })?;
    let run_path = session.runs().join(run_id.as_str());
    match fs::symlink_metadata(&run_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PublishedRunError::NotFound {
                session_id: session.session_id().as_str().to_owned(),
                run_id: run_id.as_str().to_owned(),
            });
        }
        Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        Ok(metadata)
            if !metadata.file_type().is_dir()
                || metadata.file_type().is_symlink()
                || metadata_is_reparse_point(&metadata) =>
        {
            return Err(PublishedRunError::InvalidLayout);
        }
        Ok(_) => {}
    }
    let run_path = canonical_direct_directory(&run_path, session.runs(), run_id.as_str())?;
    let manifest_path = run_path.join("manifest.json");
    match fs::symlink_metadata(&manifest_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PublishedRunError::NotFound {
                session_id: session.session_id().as_str().to_owned(),
                run_id: run_id.as_str().to_owned(),
            });
        }
        Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        Ok(metadata)
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata_is_reparse_point(&metadata) =>
        {
            return Err(PublishedRunError::InvalidLayout);
        }
        Ok(_) => {}
    }
    let manifest_path = canonical_direct_file(&manifest_path, &run_path, "manifest.json")?;
    let bytes = fs::read(manifest_path).map_err(PublishedRunError::ReadManifest)?;
    let manifest: RunManifest =
        serde_json::from_slice(&bytes).map_err(PublishedRunError::DecodeManifest)?;
    validate(&manifest, session.session_id().as_str(), run_id.as_str())?;
    let output_paths = resolve_outputs(&run_path, &manifest)?;
    Ok(PublishedRun {
        run_id: manifest.run_id,
        pack: manifest.pack,
        workflow: manifest.workflow,
        outputs: manifest.outputs,
        output_paths,
    })
}

pub(super) fn resolve_all(session: &SessionLayout) -> Result<Vec<PublishedRun>, PublishedRunError> {
    let entries = fs::read_dir(session.runs()).map_err(PublishedRunError::ReadRuns)?;
    let mut selected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(PublishedRunError::ReadRuns)?;
        let path = entry.path();
        let Some(metadata) = optional_unselected_candidate(fs::symlink_metadata(&path))? else {
            continue;
        };
        if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
            return Err(PublishedRunError::InvalidLayout);
        }
        if !metadata.file_type().is_dir() {
            continue;
        }
        let Some(canonical) = optional_unselected_candidate(dunce::canonicalize(&path))? else {
            continue;
        };
        if canonical.parent() != Some(session.runs()) {
            return Err(PublishedRunError::InvalidLayout);
        }
        match fs::symlink_metadata(canonical.join("manifest.json")) {
            Ok(_) => {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| PublishedRunError::InvalidLayout)?;
                selected.push(name);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(PublishedRunError::CorruptPath(error)),
        }
    }
    selected.sort();
    let runs = selected
        .into_iter()
        .map(|run_id| resolve(session, &run_id))
        .collect::<Result<Vec<_>, _>>()?;
    if runs.is_empty() {
        return Err(PublishedRunError::NoPublishedRuns);
    }
    Ok(runs)
}

fn optional_unselected_candidate<T>(result: io::Result<T>) -> Result<Option<T>, PublishedRunError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PublishedRunError::CorruptPath(error)),
    }
}

fn resolve_outputs(
    run_path: &Path,
    manifest: &RunManifest,
) -> Result<BTreeMap<String, String>, PublishedRunError> {
    let output_directory =
        canonical_direct_directory(&run_path.join("outputs"), run_path, "outputs")?;
    manifest
        .outputs
        .keys()
        .map(|name| {
            let file_name = format!("{name}.parquet");
            let output = canonical_direct_file(
                &output_directory.join(&file_name),
                &output_directory,
                &file_name,
            )?;
            let path = output
                .to_str()
                .map(str::to_owned)
                .ok_or(PublishedRunError::NonUnicodePath)?;
            Ok((name.clone(), path))
        })
        .collect()
}

fn canonical_direct_directory(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, PublishedRunError> {
    let metadata = fs::symlink_metadata(path).map_err(PublishedRunError::CorruptPath)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    let canonical = dunce::canonicalize(path).map_err(PublishedRunError::CorruptPath)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    Ok(canonical)
}

fn canonical_direct_file(
    path: &Path,
    parent: &Path,
    name: &str,
) -> Result<PathBuf, PublishedRunError> {
    let metadata = fs::symlink_metadata(path).map_err(PublishedRunError::CorruptPath)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    let canonical = dunce::canonicalize(path).map_err(PublishedRunError::CorruptPath)?;
    if canonical.parent() != Some(parent)
        || canonical.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    Ok(canonical)
}

fn validate(
    manifest: &RunManifest,
    session_id: &str,
    run_id: &str,
) -> Result<(), PublishedRunError> {
    if manifest.session_id != session_id
        || manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
    {
        return Err(PublishedRunError::InvalidFacts);
    }
    if manifest.inputs.iter().any(|(name, value)| {
        name.is_empty()
            || !matches!(
                value,
                serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::String(_)
            )
            || value.as_f64().is_some_and(|number| !number.is_finite())
    }) {
        return Err(PublishedRunError::InvalidFacts);
    }
    for (name, output) in &manifest.outputs {
        if !workflow_runtime::valid_output_name(name)
            || output
                .columns
                .iter()
                .any(|column| column.name.is_empty() || column.data_type.trim().is_empty())
        {
            return Err(PublishedRunError::InvalidFacts);
        }
    }
    Ok(())
}

fn diagnostic_safe_argument(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            rendered.extend(character.escape_default());
        } else {
            rendered.push(character);
        }
    }
    rendered
}

#[derive(Debug, Error, Diagnostic)]
pub(super) enum PublishedRunError {
    #[error("Run {run_id} does not exist in Analysis Session {session_id}")]
    #[diagnostic(help(
        "Use the exact Session ID and Run ID returned by the same successful `kat run`"
    ))]
    NotFound { session_id: String, run_id: String },
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    CorruptPath(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidLayout,
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    ReadManifest(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    DecodeManifest(#[source] serde_json::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidFacts,
    #[error("Analysis Session has no published Runs")]
    #[diagnostic(help("Use a Session returned by a successful `kat run`"))]
    NoPublishedRuns,
    #[error("Run storage could not be enumerated")]
    ReadRuns(#[source] io::Error),
    #[error("Run path cannot be represented as native Unicode")]
    NonUnicodePath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unselected_candidate_disappearance_is_ignored() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("disappeared-run");

        assert!(
            optional_unselected_candidate(fs::symlink_metadata(&missing))
                .unwrap()
                .is_none()
        );
        assert!(
            optional_unselected_candidate(dunce::canonicalize(&missing))
                .unwrap()
                .is_none()
        );
    }
}
