use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Args;
use miette::Diagnostic;
use serde::Serialize;
use thiserror::Error;

use crate::{
    SkillRootError, locate_data_home, locate_skill_root,
    operation_log::{OperationLog, OperationLogError},
    pack_discovery::{self, PackDiscoveryPaths},
    response,
    run_manifest::{self, RunManifest},
    session_store::{RunAllocation, RunId, SessionStore, SessionStoreError},
    text_projection::project_inline_text,
    workflow_runtime,
};

mod nested;

use nested::NestedRunCoordinator;
pub(crate) use nested::TestRunCoordinator;

#[derive(Args)]
pub(super) struct RunArgs {
    /// Continue one exact published Analysis Session.
    #[arg(long, value_name = "SESSION_ID")]
    session: String,
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Workflow name from the PACK production Interface.
    #[arg(long, value_name = "NAME")]
    workflow: String,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add a PACK directory for this command. Repeat to add more PACKs."
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
/// Output rows are addressed by the Session ID, Run ID, and Output name, not a physical path.
#[derive(Serialize)]
pub(super) struct RunResult {
    session_id: String,
    run_id: String,
    outputs: BTreeMap<String, PublicOutput>,
}

/// Public metadata for one named Run Output.
#[derive(Serialize)]
struct PublicOutput {
    columns: Vec<workflow_runtime::Column>,
    row_count: u64,
}

fn public_result(manifest: &RunManifest) -> RunResult {
    RunResult {
        session_id: manifest.session_id.clone(),
        run_id: manifest.run_id.clone(),
        outputs: manifest
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

pub(super) fn execute(arguments: RunArgs) -> response::PreparedResponse<RunResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let run_id = RunId::generate();
    let candidate_id = run_id.as_str().to_owned();
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
    let store = SessionStore::new(&data_home);
    let mut allocation = match store.create_run_in(&arguments.session, run_id) {
        Ok(allocation) => allocation,
        Err(error) => {
            let prepared = finish_failure(log, RunOperationError::SessionStore(error.error));
            return match error.lease {
                Some(lease) => response::retain_session_lease(prepared, lease),
                None => prepared,
            };
        }
    };
    let session_log =
        project_inline_text(&format!("{:?}", allocation.layout().session_id().as_str()));
    if let Err(error) = log.append(format!("session: {session_log}\n").as_bytes()) {
        let prepared = log_failure(error);
        return response::retain_session_lease(prepared, allocation.into_lease());
    }
    let prepared = execute_allocated_run(&data_home, log, arguments, &mut allocation);
    response::retain_session_lease(prepared, allocation.into_lease())
}

fn execute_allocated_run(
    data_home: &Path,
    mut log: OperationLog,
    arguments: RunArgs,
    allocation: &mut RunAllocation,
) -> response::PreparedResponse<RunResult> {
    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => {
            return finish_failure(log, RunOperationError::SkillRoot(source));
        }
    };
    let discovery_paths = PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: arguments.pack_directories,
    };
    let discovered = match pack_discovery::discover(discovery_paths.clone()) {
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
    let Some(pack_path) = pack.directory().to_str().map(str::to_owned) else {
        return finish_failure(
            log,
            RunOperationError::NonUnicodePath {
                label: "PACK",
                path: pack.directory().to_path_buf(),
            },
        );
    };
    let Some(candidate_path) = allocation.candidate().to_str().map(str::to_owned) else {
        return finish_failure(log, RunOperationError::PrivateCandidatePath);
    };
    let Some(datasource_root) = allocation
        .layout()
        .materializations()
        .to_str()
        .map(str::to_owned)
    else {
        return finish_failure(log, RunOperationError::PrivateDatasourceRootPath);
    };
    let Some(scratch_root) = allocation.scratch().to_str().map(str::to_owned) else {
        return finish_failure(log, RunOperationError::PrivateScratchRootPath);
    };
    let pack_path_log = project_inline_text(&format!("{:?}", pack.directory()));
    let arguments_log = project_inline_text(&format!("{:?}", arguments.workflow_arguments));
    if let Err(error) =
        log.append(format!("pack_path: {pack_path_log}\narguments: {arguments_log}\n").as_bytes())
    {
        return log_failure(error);
    }

    let coordinator = Arc::new(NestedRunCoordinator::for_root(
        data_home.to_path_buf(),
        allocation.layout().session_id().as_str().to_owned(),
        discovery_paths,
        pack.name().to_owned(),
        arguments.workflow.clone(),
    ));
    let outcome = workflow_runtime::execute_workflow_runtime_with_nested(
        log,
        workflow_runtime::RunWorkflowInvocation {
            session_id: allocation.layout().session_id().as_str().to_owned(),
            pack_name: pack.name().to_owned(),
            pack_path,
            workflow_name: arguments.workflow.clone(),
            arguments: arguments.workflow_arguments,
            candidate_id: allocation.run_id().as_str().to_owned(),
            candidate_path,
            datasource_root,
            scratch_root,
        },
        coordinator.clone(),
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

    if let Err(error) = allocation.clean_scratch() {
        return finish_failure(log, RunOperationError::SessionStore(error));
    }
    if let Err(source) =
        run_manifest::validate_candidate_outputs(allocation.candidate(), &runtime.outputs)
    {
        return finish_failure(log, RunOperationError::InvalidOutputLayout { source });
    }

    let child_runs = match coordinator.child_runs() {
        Ok(child_runs) => child_runs,
        Err(source) => {
            return finish_failure(log, RunOperationError::ChildRunLedger { source });
        }
    };
    let manifest = RunManifest::new(
        allocation.layout().session_id().as_str().to_owned(),
        allocation.run_id().as_str().to_owned(),
        pack.name().to_owned(),
        arguments.workflow,
        child_runs,
        runtime.effective_inputs,
        runtime.outputs,
    );
    let result = public_result(&manifest);
    if let Err(error) = log.append(b"publication_gate: ready\n") {
        return log_failure(error);
    }
    let log_path = match log.finish() {
        Ok(log_path) => log_path,
        Err(error) => return log_failure(error),
    };
    if let Err(error) = publish_run_manifest(allocation.candidate(), &manifest) {
        return response::prepare_cli_failure_with_log(miette::Report::new(error), Some(log_path));
    }
    allocation.mark_run_published();
    response::prepare_success_with_log(result, Some(log_path))
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
    #[error(transparent)]
    #[diagnostic(transparent)]
    SessionStore(#[from] SessionStoreError),
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
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
    #[error("private Datasource root path is not representable as native Unicode")]
    PrivateDatasourceRootPath,
    #[error("private scratch root path is not representable as native Unicode")]
    PrivateScratchRootPath,
    #[error("Workflow Runtime produced an invalid Run Output layout")]
    #[diagnostic(help("Inspect the Operation log and repair the Workflow Output writer"))]
    InvalidOutputLayout {
        #[source]
        source: run_manifest::PublishedRunError,
    },
    #[error("nested Workflow child Run ledger is unavailable")]
    #[diagnostic(help("Inspect the Operation log and retry the complete Run"))]
    ChildRunLedger {
        #[source]
        source: nested::ChildRunLedgerError,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Operation};

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
            "--session",
            "019f6e00-0000-7000-8000-000000000000",
            "--pack",
            "alpha",
            "--workflow",
            "analyze",
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
                "--session",
                "019f6e00-0000-7000-8000-000000000000",
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
    fn parser_requires_a_session_selector() {
        assert!(
            Cli::try_parse_from(["kat", "run", "--pack", "alpha", "--workflow", "analyze",])
                .is_err()
        );
    }

    #[test]
    fn premature_manifest_is_removed_and_never_accepted_as_publication() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("manifest.json"), "runtime-owned").unwrap();
        let manifest = RunManifest::new(
            "019f6e00-0000-7000-8000-000000000000".to_owned(),
            "019f6e00-0000-7000-8000-000000000001".to_owned(),
            "alpha".to_owned(),
            "analyze".to_owned(),
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::from([(
                "main".to_owned(),
                workflow_runtime::RunOutputMetadata {
                    columns: Vec::new(),
                    row_count: 0,
                },
            )]),
        );

        assert!(matches!(
            publish_run_manifest(temporary.path(), &manifest),
            Err(RunOperationError::PrematureManifest)
        ));
        assert!(!temporary.path().join("manifest.json").exists());
    }

    #[test]
    fn published_run_manifest_has_no_dataset_field() {
        let temporary = tempfile::tempdir().unwrap();
        let manifest = RunManifest::new(
            "019f6e00-0000-7000-8000-000000000010".to_owned(),
            "019f6e00-0000-7000-8000-000000000011".to_owned(),
            "alpha".to_owned(),
            "analyze".to_owned(),
            vec![
                "019f6e00-0000-7000-8000-000000000013".to_owned(),
                "019f6e00-0000-7000-8000-000000000012".to_owned(),
            ],
            BTreeMap::new(),
            BTreeMap::new(),
        );

        publish_run_manifest(temporary.path(), &manifest).unwrap();

        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(temporary.path().join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            document,
            serde_json::json!({
                "session_id": "019f6e00-0000-7000-8000-000000000010",
                "run_id": "019f6e00-0000-7000-8000-000000000011",
                "pack": "alpha",
                "workflow": "analyze",
                "child_runs": [
                    "019f6e00-0000-7000-8000-000000000012",
                    "019f6e00-0000-7000-8000-000000000013"
                ],
                "inputs": {},
                "outputs": {}
            })
        );
    }
}
