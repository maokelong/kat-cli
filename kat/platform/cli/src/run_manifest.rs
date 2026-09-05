use std::{
    collections::{BTreeMap, BTreeSet},
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
    pub(super) child_runs: Vec<String>,
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
        mut child_runs: Vec<String>,
        inputs: BTreeMap<String, serde_json::Value>,
        outputs: BTreeMap<String, RunOutputMetadata>,
    ) -> Self {
        child_runs.sort();
        Self {
            session_id,
            run_id,
            pack,
            workflow,
            child_runs,
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
    pub(super) child_runs: Vec<String>,
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
        child_runs: manifest.child_runs,
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
    complete_child_run_references(session, runs)
}

fn complete_child_run_references(
    session: &SessionLayout,
    runs: Vec<PublishedRun>,
) -> Result<Vec<PublishedRun>, PublishedRunError> {
    let mut pending = runs
        .iter()
        .flat_map(|run| run.child_runs.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut published = runs
        .into_iter()
        .map(|run| (run.run_id.clone(), run))
        .collect::<BTreeMap<_, _>>();
    while let Some(child_run_id) = pending.pop_first() {
        if published.contains_key(&child_run_id) {
            continue;
        }
        let child = match resolve(session, &child_run_id) {
            Ok(child) => child,
            Err(PublishedRunError::NotFound { .. }) => {
                return Err(PublishedRunError::InvalidFacts);
            }
            Err(error) => return Err(error),
        };
        pending.extend(child.child_runs.iter().cloned());
        published.insert(child_run_id, child);
    }
    Ok(published.into_values().collect())
}

/// Verifies the complete Runtime-owned Output directory before the Host publishes a Manifest.
pub(super) fn validate_candidate_outputs(
    candidate: &Path,
    outputs: &BTreeMap<String, RunOutputMetadata>,
) -> Result<(), PublishedRunError> {
    resolve_output_paths(candidate, outputs).map(drop)
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
    resolve_output_paths(run_path, &manifest.outputs)
}

fn resolve_output_paths(
    run_path: &Path,
    outputs: &BTreeMap<String, RunOutputMetadata>,
) -> Result<BTreeMap<String, String>, PublishedRunError> {
    let output_directory =
        canonical_direct_directory(&run_path.join("outputs"), run_path, "outputs")?;
    let expected = outputs
        .keys()
        .map(|name| format!("{name}.parquet").into())
        .collect::<BTreeSet<_>>();
    let observed = fs::read_dir(&output_directory)
        .map_err(PublishedRunError::CorruptPath)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(PublishedRunError::CorruptPath)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if observed != expected {
        return Err(PublishedRunError::InvalidLayout);
    }
    outputs
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
        || manifest
            .child_runs
            .iter()
            .any(|child_run| child_run == run_id || RunId::parse(child_run).is_none())
        || !manifest.child_runs.windows(2).all(|pair| pair[0] < pair[1])
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
    #[error("Run storage could not be enumerated")]
    ReadRuns(#[source] io::Error),
    #[error("Run path cannot be represented as native Unicode")]
    NonUnicodePath,
}

#[cfg(test)]
mod tests {
    use std::{fs::File, sync::Arc};

    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use super::*;

    fn one_output() -> BTreeMap<String, RunOutputMetadata> {
        BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: Vec::new(),
                row_count: 0,
            },
        )])
    }

    fn typed_output(data_type: &str, row_count: u64) -> BTreeMap<String, RunOutputMetadata> {
        BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: vec![workflow_runtime::Column {
                    name: "value".to_owned(),
                    data_type: data_type.to_owned(),
                }],
                row_count,
            },
        )])
    }

    fn write_empty_parquet(path: &Path, data_type: DataType) {
        write_empty_parquet_schema(path, vec![Field::new("value", data_type, true)]);
    }

    fn write_empty_parquet_schema(path: &Path, fields: Vec<Field>) {
        let schema = Arc::new(Schema::new(fields));
        ArrowWriter::try_new(File::create(path).unwrap(), schema, None)
            .unwrap()
            .close()
            .unwrap();
    }

    fn canonical_temporary_directory() -> (tempfile::TempDir, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        // SessionStore passes canonical candidate paths in production. Hosted Windows
        // runners may expose TEMP through an 8.3 alias, so fixtures must do the same.
        let canonical = dunce::canonicalize(temporary.path()).unwrap();
        (temporary, canonical)
    }

    fn publish_test_run(
        store: &crate::session_store::SessionStore,
        session_id: &str,
        run_id: RunId,
        child_runs: Vec<String>,
    ) {
        let mut allocation = match store.create_run_in(session_id, run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create Run allocation"),
        };
        let outputs = allocation.candidate().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);
        let manifest = RunManifest::new(
            session_id.to_owned(),
            run_id.as_str().to_owned(),
            "test-pack".to_owned(),
            "test-workflow".to_owned(),
            child_runs,
            BTreeMap::new(),
            typed_output("int64", 0),
        );
        fs::write(
            allocation.candidate().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        allocation.mark_run_published();
    }

    #[test]
    fn candidate_output_gate_accepts_the_exact_owned_file_set() {
        let (_temporary, candidate) = canonical_temporary_directory();
        let outputs = candidate.join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);

        validate_candidate_outputs(&candidate, &typed_output("int64", 0)).unwrap();
    }

    #[test]
    fn candidate_output_gate_rejects_an_undeclared_direct_entry() {
        let (_temporary, candidate) = canonical_temporary_directory();
        let outputs = candidate.join("outputs");
        fs::create_dir(&outputs).unwrap();
        fs::write(outputs.join("main.parquet"), b"declared").unwrap();
        fs::write(outputs.join("extra.parquet"), b"undeclared").unwrap();

        assert!(matches!(
            validate_candidate_outputs(&candidate, &one_output()),
            Err(PublishedRunError::InvalidLayout)
        ));
    }

    #[test]
    fn published_inventory_uses_manifest_facts_without_reading_parquet_content() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str().to_owned();
        drop(opened);
        let run_id = RunId::generate();
        let mut allocation = match store.create_run_in(&session_id, run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create Run allocation"),
        };
        let outputs = allocation.candidate().join("outputs");
        fs::create_dir(&outputs).unwrap();
        write_empty_parquet(&outputs.join("main.parquet"), DataType::Int64);
        let manifest = RunManifest::new(
            session_id,
            run_id.as_str().to_owned(),
            "test-pack".to_owned(),
            "test-workflow".to_owned(),
            Vec::new(),
            BTreeMap::new(),
            typed_output("int64", 0),
        );
        fs::write(
            allocation.candidate().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        allocation.mark_run_published();
        fs::write(outputs.join("main.parquet"), b"not parquet").unwrap();

        let published = resolve(allocation.layout(), run_id.as_str()).unwrap();
        assert_eq!(published.outputs["main"].row_count, 0);
        assert_eq!(published.outputs["main"].columns[0].data_type, "int64");
        assert!(published.output_paths.contains_key("main"));
    }

    #[test]
    fn resolve_all_accepts_a_published_direct_child() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let child_run_id = RunId::generate();
        let parent_run_id = RunId::generate();
        publish_test_run(&store, session_id, child_run_id.clone(), Vec::new());
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![child_run_id.as_str().to_owned()],
        );

        assert_eq!(resolve_all(opened.layout()).unwrap().len(), 2);
    }

    #[test]
    fn child_completion_adds_published_descendants_omitted_from_the_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let grandchild_run_id = RunId::generate();
        let child_run_id = RunId::generate();
        let parent_run_id = RunId::generate();
        publish_test_run(&store, session_id, grandchild_run_id.clone(), Vec::new());
        publish_test_run(
            &store,
            session_id,
            child_run_id.clone(),
            vec![grandchild_run_id.as_str().to_owned()],
        );
        publish_test_run(
            &store,
            session_id,
            parent_run_id.clone(),
            vec![child_run_id.as_str().to_owned()],
        );
        let parent = resolve(opened.layout(), parent_run_id.as_str()).unwrap();

        let completed = complete_child_run_references(opened.layout(), vec![parent]).unwrap();
        assert_eq!(
            completed
                .into_iter()
                .map(|run| run.run_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                parent_run_id.as_str().to_owned(),
                child_run_id.as_str().to_owned(),
                grandchild_run_id.as_str().to_owned(),
            ])
        );
    }

    #[test]
    fn resolve_rejects_a_self_referencing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            run_id.clone(),
            vec![run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve(opened.layout(), run_id.as_str()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_a_self_referencing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            run_id.clone(),
            vec![run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_a_missing_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let parent_run_id = RunId::generate();
        let missing_child_run_id = RunId::generate();
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![missing_child_run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

    #[test]
    fn resolve_all_rejects_an_unpublished_child_run() {
        let temporary = tempfile::tempdir().unwrap();
        let store = crate::session_store::SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str();
        let parent_run_id = RunId::generate();
        let unpublished_child_run_id = RunId::generate();
        let _unpublished = match store.create_run_in(session_id, unpublished_child_run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create unpublished Run allocation"),
        };
        publish_test_run(
            &store,
            session_id,
            parent_run_id,
            vec![unpublished_child_run_id.as_str().to_owned()],
        );

        assert!(matches!(
            resolve_all(opened.layout()),
            Err(PublishedRunError::InvalidFacts)
        ));
    }

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
