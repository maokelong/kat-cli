use std::{
    fs, io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use thiserror::Error;

use crate::{run::RunManifest, workflow_runtime};

pub(super) struct PublishedRun {
    pub(super) path: PathBuf,
    pub(super) manifest: RunManifest,
}

pub(super) fn read(data_home: &Path, run_id: &str) -> Result<PublishedRun, PublishedRunError> {
    uuid::Uuid::parse_str(run_id)
        .ok()
        .filter(|identity| identity.get_version_num() == 7 && identity.to_string() == run_id)
        .ok_or_else(|| PublishedRunError::NotFound {
            run_id: diagnostic_safe_argument(run_id),
        })?;
    let runs = data_home.join("runs");
    let candidate = runs.join(run_id);
    let manifest_path = candidate.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(PublishedRunError::NotFound {
            run_id: run_id.to_owned(),
        });
    }
    if manifest_path.is_symlink() {
        return Err(PublishedRunError::InvalidLayout);
    }
    let run_path = dunce::canonicalize(&candidate).map_err(PublishedRunError::CorruptPath)?;
    let runs_path = dunce::canonicalize(&runs).map_err(PublishedRunError::CorruptPath)?;
    if run_path.parent() != Some(runs_path.as_path())
        || run_path.file_name().and_then(|name| name.to_str()) != Some(run_id)
        || !run_path.is_dir()
    {
        return Err(PublishedRunError::InvalidLayout);
    }
    let bytes =
        fs::read(run_path.join("manifest.json")).map_err(PublishedRunError::ReadManifest)?;
    let manifest: RunManifest =
        serde_json::from_slice(&bytes).map_err(PublishedRunError::DecodeManifest)?;
    validate(&manifest, run_id)?;
    Ok(PublishedRun {
        path: run_path,
        manifest,
    })
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

fn validate(manifest: &RunManifest, run_id: &str) -> Result<(), PublishedRunError> {
    if manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
        || manifest
            .dataset
            .as_ref()
            .is_some_and(|path| path.is_empty() || !Path::new(path).is_absolute())
    {
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
    if manifest.inputs.iter().any(|(name, value)| {
        name.is_empty()
            || !matches!(
                value,
                serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::String(_)
            )
    }) {
        return Err(PublishedRunError::InvalidFacts);
    }
    Ok(())
}

#[derive(Debug, Error, Diagnostic)]
pub(super) enum PublishedRunError {
    #[error("Run {run_id} does not exist")]
    #[diagnostic(help("Use the exact Run ID returned by a successful `kat run`"))]
    NotFound { run_id: String },
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
}
