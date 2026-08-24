use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use clap::Args;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    SkillRootError, locate_data_home, locate_skill_root,
    operation_log::{OperationLog, OperationLogError},
    pack_discovery::{self, PackDiscoveryPaths},
    response,
    text_projection::project_inline_text,
    workflow_runtime,
};

#[derive(Args)]
pub(super) struct RunArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Workflow name from the PACK production Interface.
    #[arg(long, value_name = "NAME")]
    workflow: String,
    /// Provide one KAT Dataset directory for this execution.
    #[arg(long, value_name = "DIRECTORY")]
    dataset: Option<PathBuf>,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add a PACK candidate directory for this command. Repeat to add more candidate directories."
    )]
    pack_directories: Vec<PathBuf>,
    /// Forward all tokens after `--` unchanged to the Workflow Input Compiler.
    /// The Operation log may retain the complete vector.
    #[arg(last = true, value_name = "ARGUMENT")]
    workflow_arguments: Vec<String>,
}

/// Metadata-only projection returned after the Run Manifest is published.
///
/// `run_id` publishes the identity for later Run operations; this slice returns
/// Run metadata only.
/// Output rows are addressed by the Run ID and Output name, not a physical path.
#[derive(Serialize)]
pub(super) struct RunResult {
    run_id: String,
    outputs: BTreeMap<String, PublicOutput>,
}

/// Public metadata for one named Run Output.
#[derive(Serialize)]
struct PublicOutput {
    columns: Vec<workflow_runtime::Column>,
    row_count: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunManifest {
    pub(super) run_id: String,
    pub(super) pack: String,
    pub(super) workflow: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_null_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) dataset: Option<String>,
    pub(super) inputs: BTreeMap<String, serde_json::Value>,
    pub(super) outputs: BTreeMap<String, workflow_runtime::RunOutputMetadata>,
}

fn deserialize_optional_non_null_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

impl RunManifest {
    fn new(
        candidate_id: String,
        pack: String,
        workflow: String,
        dataset: Option<String>,
        runtime: workflow_runtime::RunWorkflowReport,
    ) -> Self {
        Self {
            run_id: candidate_id,
            pack,
            workflow,
            dataset,
            inputs: runtime.effective_inputs,
            outputs: runtime.outputs,
        }
    }

    fn public_result(&self) -> RunResult {
        RunResult {
            run_id: self.run_id.clone(),
            outputs: self
                .outputs
                .iter()
                .map(|(name, output)| {
                    (
                        name.clone(),
                        PublicOutput {
                            columns: output.columns.clone(),
                            row_count: output.row_count,
                        },
                    )
                })
                .collect(),
        }
    }
}

pub(super) fn execute(arguments: RunArgs) -> response::PreparedResponse<RunResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let candidate_id = uuid::Uuid::now_v7().to_string();
    let pack_log = project_inline_text(&arguments.pack);
    let workflow_log = project_inline_text(&arguments.workflow);
    let mut log = match OperationLog::create_run(&data_home, &candidate_id, |file| {
        writeln!(
            file,
            "operation: kat run\nscope: CLI preparation and Runtime execution\n\
             publication: manifest.json is the only published Run fact\n\
             candidate: {candidate_id}\npack: {}\nworkflow: {}",
            pack_log, workflow_log
        )
    }) {
        Ok(log) => log,
        Err(error) => return log_failure(error),
    };
    let mut candidate = match create_run_candidate(&data_home, &candidate_id) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => {
            return finish_failure(log, RunOperationError::SkillRoot(source));
        }
    };
    let discovered = match pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: arguments.pack_directories,
    }) {
        Ok(discovered) => discovered,
        Err(source) => {
            return finish_failure(log, RunOperationError::Discovery { source });
        }
    };
    let Some(pack) = discovered.get(&arguments.pack) else {
        return finish_failure(
            log,
            RunOperationError::UnknownPack {
                name: arguments.pack,
            },
        );
    };
    let mut pack_paths = BTreeMap::new();
    for discovered_pack in discovered.iter() {
        let Some(path) = discovered_pack.directory().to_str() else {
            return finish_failure(
                log,
                RunOperationError::NonUnicodePath {
                    label: "discovered PACK",
                    path: discovered_pack.directory().to_path_buf(),
                },
            );
        };
        pack_paths.insert(discovered_pack.name().to_owned(), path.to_owned());
    }
    let dataset = match arguments.dataset {
        Some(path) => match kat_datasource::resolve_dataset(&path) {
            Ok(dataset) => Some(dataset),
            Err(source) => {
                return finish_failure(log, RunOperationError::Dataset { source });
            }
        },
        None => None,
    };
    let runtime_dataset = match dataset
        .as_ref()
        .map(workflow_runtime::project_dataset)
        .transpose()
    {
        Ok(dataset) => dataset,
        Err(error) => {
            return finish_failure(
                log,
                RunOperationError::NonUnicodePath {
                    label: error.label,
                    path: error.path,
                },
            );
        }
    };
    let dataset_path = runtime_dataset.as_ref().map(|dataset| dataset.path.clone());
    let Some(pack_path) = pack.directory().to_str().map(str::to_owned) else {
        return finish_failure(
            log,
            RunOperationError::NonUnicodePath {
                label: "PACK",
                path: pack.directory().to_path_buf(),
            },
        );
    };
    let Some(candidate_path) = candidate.path().to_str().map(str::to_owned) else {
        return finish_failure(log, RunOperationError::PrivateCandidatePath);
    };
    let pack_path_log = project_inline_text(&format!("{:?}", pack.directory()));
    let dataset_log = project_inline_text(dataset_path.as_deref().unwrap_or("not provided"));
    let arguments_log = project_inline_text(&format!("{:?}", arguments.workflow_arguments));
    if let Err(error) = log.append(
        format!("pack_path: {pack_path_log}\ndataset: {dataset_log}\narguments: {arguments_log}\n")
            .as_bytes(),
    ) {
        return log_failure(error);
    }

    let outcome = workflow_runtime::execute_workflow_runtime(
        log,
        workflow_runtime::RunWorkflowInvocation {
            pack_name: pack.name().to_owned(),
            pack_path,
            pack_paths,
            workflow_name: arguments.workflow.clone(),
            dataset: runtime_dataset,
            arguments: arguments.workflow_arguments,
            candidate_id: candidate_id.clone(),
            candidate_path,
        },
    );
    let (runtime, mut log) = match outcome {
        Ok(workflow_runtime::RunWorkflowOutcome::Success { result, log }) => (result, log),
        Ok(workflow_runtime::RunWorkflowOutcome::Failure {
            diagnostic,
            log_path,
        }) => return response::prepare_runtime_failure(diagnostic, log_path),
        Err(error) => {
            let log_path = error.log_path();
            return response::prepare_cli_failure_with_log(miette::Report::new(error), log_path);
        }
    };

    let manifest = RunManifest::new(
        candidate_id,
        pack.name().to_owned(),
        arguments.workflow,
        dataset_path,
        runtime,
    );
    let result = manifest.public_result();
    if let Err(error) = log.append(b"publication_gate: ready\n") {
        return log_failure(error);
    }
    let log_path = match log.finish() {
        Ok(log_path) => log_path,
        Err(error) => return log_failure(error),
    };
    if let Err(error) = candidate.publish(&manifest) {
        return response::prepare_cli_failure_with_log(miette::Report::new(error), Some(log_path));
    }
    response::prepare_success_with_log(result, Some(log_path))
}

/// Owns a private candidate until the manifest makes it a published Run.
///
/// Cleanup is intentionally best effort: an externally terminated process may
/// leave cache data, but a directory without the manifest is never Run state.
struct RunCandidate {
    path: PathBuf,
    published: bool,
}

impl RunCandidate {
    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(&mut self, manifest: &RunManifest) -> Result<(), RunOperationError> {
        publish_run_manifest(&self.path, manifest)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for RunCandidate {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn create_run_candidate(data_home: &Path, id: &str) -> Result<RunCandidate, RunOperationError> {
    let runs = data_home.join("runs");
    fs::create_dir_all(&runs).map_err(|source| RunOperationError::CreateRuns {
        path: runs.clone(),
        source,
    })?;
    let path = runs.join(id);
    fs::create_dir(&path).map_err(|source| RunOperationError::CreateCandidate {
        path: path.clone(),
        source,
    })?;
    let mut candidate = RunCandidate {
        path,
        published: false,
    };
    let unresolved = candidate.path.clone();
    candidate.path = dunce::canonicalize(&unresolved).map_err(|source| {
        RunOperationError::CanonicalCandidate {
            path: unresolved,
            source,
        }
    })?;
    Ok(candidate)
}

fn publish_run_manifest(candidate: &Path, manifest: &RunManifest) -> Result<(), RunOperationError> {
    let destination = candidate.join("manifest.json");
    if destination.exists() {
        fs::remove_file(&destination)
            .map_err(|source| RunOperationError::RemovePrematureManifest { source })?;
        return Err(RunOperationError::PrematureManifest);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(candidate).map_err(|source| {
        RunOperationError::CreateManifestCandidate {
            path: candidate.to_path_buf(),
            source,
        }
    })?;
    serde_json::to_writer(temporary.as_file_mut(), manifest)
        .map_err(RunOperationError::EncodeManifest)?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .map_err(RunOperationError::WriteManifest)?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(RunOperationError::FlushManifest)?;
    temporary.persist_noclobber(&destination).map_err(|error| {
        RunOperationError::PublishManifest {
            path: destination,
            source: error.error,
        }
    })?;
    Ok(())
}

fn finish_failure(
    mut log: OperationLog,
    error: RunOperationError,
) -> response::PreparedResponse<RunResult> {
    let error_text = project_inline_text(&error.to_string());
    if let Err(log_error) = log.append(format!("status: failure\nerror: {error_text}\n").as_bytes())
    {
        return log_failure(log_error);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(error) => log_failure(error),
    }
}

fn log_failure(error: OperationLogError) -> response::PreparedResponse<RunResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        RunOperationError::IncompleteOperationLog(error)
    } else {
        RunOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

#[derive(Debug, Error, Diagnostic)]
enum RunOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("failed to create Run root {path}")]
    CreateRuns {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create private Run candidate")]
    #[diagnostic(help("Provide writable KAT Data Home storage and retry"))]
    CreateCandidate {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve private Run candidate")]
    CanonicalCandidate {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Run Operation log could not be delivered")]
    #[diagnostic(help("Provide writable KAT Data Home storage and retry the complete Run"))]
    OperationLog(#[source] OperationLogError),
    #[error("Run Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(#[source] OperationLogError),
    #[error("PACK discovery failed")]
    #[diagnostic(help("Correct the first invalid PACK candidate and retry"))]
    Discovery {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
    #[error("PACK {name:?} was not discovered")]
    #[diagnostic(help(
        "Use the exact manifest name from `kat inspect`, or add its directory with --pack-dir"
    ))]
    UnknownPack { name: String },
    #[error("Dataset resolution failed")]
    #[diagnostic(help("Provide a complete KAT Dataset directory or omit --dataset"))]
    Dataset {
        #[source]
        source: kat_datasource::DatasetInspectionError,
    },
    #[error("{label} path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { label: &'static str, path: PathBuf },
    #[error("failed to create a temporary Run Manifest")]
    CreateManifestCandidate {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to encode the final Run Manifest")]
    EncodeManifest(#[source] serde_json::Error),
    #[error("failed to write the final Run Manifest")]
    WriteManifest(#[source] io::Error),
    #[error("failed to durably flush the final Run Manifest")]
    FlushManifest(#[source] io::Error),
    #[error("failed to publish the final Run Manifest")]
    #[diagnostic(help("Inspect the Operation log, provide writable storage, and retry"))]
    PublishManifest {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Workflow Runtime wrote the CLI-owned final Run Manifest")]
    #[diagnostic(help("Inspect the Operation log and repair the bundled Runtime deployment"))]
    PrematureManifest,
    #[error("failed to remove a premature final Run Manifest")]
    RemovePrematureManifest {
        #[source]
        source: io::Error,
    },
    #[error("private Run candidate path is not representable as native Unicode")]
    PrivateCandidatePath,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Operation};

    #[test]
    fn unpublished_run_candidate_is_removed_when_its_owner_drops() {
        let temporary = tempfile::tempdir().unwrap();
        let candidate_path;
        {
            let candidate =
                create_run_candidate(temporary.path(), "019f6e00-0000-7000-8000-000000000008")
                    .unwrap();
            candidate_path = candidate.path().to_path_buf();
            assert!(candidate_path.is_dir());
        }

        assert!(!candidate_path.exists());
    }

    #[test]
    fn run_log_diagnostics_keep_the_io_cause_and_hide_the_private_candidate() {
        let candidate_id = "019f6e00-0000-7000-8000-000000000005";
        let error = RunOperationError::IncompleteOperationLog(OperationLogError::Write {
            path: PathBuf::from(format!(r"C:\data\logs\run-{candidate_id}.log")),
            source: io::Error::other("injected log write failure"),
        });

        let source = std::error::Error::source(&error).unwrap();
        assert_eq!(source.to_string(), "failed to write Operation log");
        assert_eq!(
            source.source().unwrap().to_string(),
            "injected log write failure"
        );
        assert!(!source.to_string().contains(candidate_id));
        assert!(!error.to_string().contains(candidate_id));
    }

    #[test]
    fn parser_forwards_workflow_arguments_only_after_separator() {
        let cli = Cli::try_parse_from([
            "kat",
            "run",
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
            "--dataset",
            "dataset",
            "--",
            "--limit",
            "5",
        ])
        .unwrap();
        let Operation::Run(arguments) = cli.operation else {
            panic!("expected run operation");
        };
        assert_eq!(arguments.workflow_arguments, ["--limit", "5"]);
        assert!(
            Cli::try_parse_from([
                "kat",
                "run",
                "--pack",
                "alpha",
                "--workflow",
                "analyze",
                "--limit",
                "5",
            ])
            .is_err()
        );
    }

    #[test]
    fn premature_manifest_is_removed_and_never_accepted_as_publication() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("manifest.json"), "runtime-owned").unwrap();
        let manifest = RunManifest {
            run_id: "019f6e00-0000-7000-8000-000000000001".to_owned(),
            pack: "alpha".to_owned(),
            workflow: "analyze".to_owned(),
            dataset: None,
            inputs: BTreeMap::new(),
            outputs: BTreeMap::from([(
                "main".to_owned(),
                workflow_runtime::RunOutputMetadata {
                    columns: Vec::new(),
                    row_count: 0,
                },
            )]),
        };

        assert!(matches!(
            publish_run_manifest(temporary.path(), &manifest),
            Err(RunOperationError::PrematureManifest)
        ));
        assert!(!temporary.path().join("manifest.json").exists());
    }
}
