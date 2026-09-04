mod configuration;
mod inspect;
mod operation_log;
mod pack_discovery;
mod query;
mod response;
mod run;
mod run_manifest;
mod session;
mod session_store;
mod test;
mod text_projection;
mod workflow_runtime;

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use miette::Diagnostic;
use operation_log::{OperationLog, OperationLogError};
use pack_discovery::{DiscoveredPack, PackDiscoveryPaths};
use serde::Serialize;
use text_projection::project_inline_text;
use thiserror::Error;

#[derive(Parser)]
#[command(name = "kat", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    operation: Operation,
}

#[derive(Subcommand)]
enum Operation {
    /// Inspect available PACKs or one PACK's Workflow and Provider knowledge.
    Inspect(inspect::InspectArgs),
    /// Execute one Workflow and atomically publish one Run.
    ///
    /// The Operation log may retain the resolved PACK path and all arguments
    /// after `--`. Do not pass secrets in these values.
    Run(run::RunArgs),
    /// Query one published Run's output.* tables and information_schema.
    ///
    /// Python/DataFusion writes Arrow's native object-row JSON mapping directly
    /// to one NDJSON result file. A successful Response returns its format,
    /// path, and column metadata; the CLI does not read or re-encode query rows.
    ///
    /// The Operation log retains the complete --sql value. Do not pass secrets
    /// in it.
    Query(query::QueryArgs),
    /// Permanently delete one Analysis Session.
    Session(session::SessionArgs),
    /// Run one PACK's pytest suite in the production execution plane.
    Test(test::TestArgs),
}

#[derive(Serialize)]
struct InspectPacksResult {
    packs: Vec<PackResult>,
}

#[derive(Serialize)]
struct PackResult {
    name: String,
    title: String,
    description: String,
    owner: String,
}

#[derive(Serialize)]
struct InspectSessionResult {
    session_id: String,
    runs: Vec<InspectSessionRun>,
}

#[derive(Serialize)]
struct InspectSessionRun {
    run_id: String,
    pack: String,
    workflow: String,
    outputs: std::collections::BTreeMap<String, workflow_runtime::RunOutputMetadata>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum InspectKnowledgeResult {
    Workflow(workflow_runtime::WorkflowInspectionResult),
    Provider(workflow_runtime::ProviderInspectionResult),
}

enum InspectKnowledgeTarget {
    Workflow(Option<String>),
    Provider(Option<String>),
}

impl InspectKnowledgeTarget {
    fn operation(&self) -> &'static str {
        match self {
            Self::Workflow(_) => "kat inspect workflow",
            Self::Provider(_) => "kat inspect provider",
        }
    }

    fn selector(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Workflow(Some(name)) => Some(("workflow", name)),
            Self::Provider(Some(name)) => Some(("provider", name)),
            Self::Workflow(None) | Self::Provider(None) => None,
        }
    }
}

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };

    match cli.operation {
        Operation::Inspect(arguments) => inspect::execute(arguments),
        Operation::Run(arguments) => response::publish(run::execute(arguments)),
        Operation::Query(arguments) => response::publish(query::execute(arguments)),
        Operation::Session(arguments) => response::publish(session::execute(arguments)),
        Operation::Test(arguments) => response::publish(test::execute(arguments)),
    }
}

fn inspect_session(session_id: String) -> response::PreparedResponse<InspectSessionResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let opened = match session_store::SessionStore::new(&data_home).open(&session_id) {
        Ok(opened) => opened,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let prepared = match run_manifest::resolve_all(opened.layout()) {
        Ok(runs) => response::prepare_success(InspectSessionResult {
            session_id: opened.layout().session_id().as_str().to_owned(),
            runs: runs
                .into_iter()
                .map(|run| InspectSessionRun {
                    run_id: run.run_id,
                    pack: run.pack,
                    workflow: run.workflow,
                    outputs: run.outputs,
                })
                .collect(),
        }),
        Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
    };
    response::retain_session_lease(prepared, opened.into_lease())
}

fn inspect_target_pack(
    pack_name: String,
    pack_directories: Vec<PathBuf>,
    target: InspectKnowledgeTarget,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let log = match OperationLog::create(&data_home, "inspect", |file| {
        writeln!(file, "operation: {}", target.operation())?;
        writeln!(file, "pack: {}", pack_name.escape_debug())?;
        if let Some((label, name)) = target.selector() {
            writeln!(file, "{label}: {}", name.escape_debug())?;
        }
        Ok(())
    }) {
        Ok(log) => log,
        Err(error) => return inspect_target_log_failure(error),
    };
    inspect_resolved_target(&data_home, log, pack_name, pack_directories, target)
}

fn inspect_run_workflow(
    session_id: String,
    run_id: String,
    pack_directories: Vec<PathBuf>,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let log = match OperationLog::create(&data_home, "inspect", |file| {
        writeln!(file, "operation: kat inspect workflow")?;
        writeln!(file, "session: {}", session_id.escape_debug())?;
        writeln!(file, "run: {}", run_id.escape_debug())
    }) {
        Ok(log) => log,
        Err(error) => return inspect_target_log_failure(error),
    };
    let opened = match session_store::SessionStore::new(&data_home).open(&session_id) {
        Ok(opened) => opened,
        Err(error) => {
            return finish_inspect_target_failure(log, InspectTargetPackError::SessionStore(error));
        }
    };
    let prepared = inspect_opened_run_workflow(&data_home, log, run_id, pack_directories, &opened);
    response::retain_session_lease(prepared, opened.into_lease())
}

fn inspect_opened_run_workflow(
    data_home: &Path,
    mut log: OperationLog,
    run_id: String,
    pack_directories: Vec<PathBuf>,
    opened: &session_store::OpenedSession,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let published_run = match run_manifest::resolve(opened.layout(), &run_id) {
        Ok(run) => run,
        Err(error) => {
            return finish_inspect_target_failure(log, InspectTargetPackError::PublishedRun(error));
        }
    };
    let pack_name = published_run.pack;
    let workflow_name = published_run.workflow;
    if let Err(error) = log.append(
        format!(
            "pack: {}\nworkflow: {}\n",
            pack_name.escape_debug(),
            workflow_name.escape_debug()
        )
        .as_bytes(),
    ) {
        return inspect_target_log_failure(error);
    }
    inspect_resolved_target(
        data_home,
        log,
        pack_name,
        pack_directories,
        InspectKnowledgeTarget::Workflow(Some(workflow_name)),
    )
}

fn inspect_resolved_target(
    data_home: &Path,
    mut log: OperationLog,
    pack_name: String,
    pack_directories: Vec<PathBuf>,
    target: InspectKnowledgeTarget,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => {
            return finish_inspect_target_failure(log, InspectTargetPackError::SkillRoot(source));
        }
    };
    let discovered = match pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: pack_directories,
    }) {
        Ok(discovered) => discovered,
        Err(source) => {
            return finish_inspect_target_failure(
                log,
                InspectTargetPackError::PackDiscovery(source.into()),
            );
        }
    };
    let Some(pack) = discovered.get(&pack_name) else {
        return finish_inspect_target_failure(
            log,
            InspectTargetPackError::UnknownPack { name: pack_name },
        );
    };
    if let Err(error) = log.append(format!("path: {:?}\n", pack.directory()).as_bytes()) {
        return inspect_target_log_failure(error);
    }

    let outcome = match target {
        InspectKnowledgeTarget::Workflow(workflow_name) => workflow_runtime::inspect_workflow(
            log,
            pack.name(),
            pack.directory(),
            workflow_name.as_deref(),
        )
        .map(|outcome| outcome.map(InspectKnowledgeResult::Workflow)),
        InspectKnowledgeTarget::Provider(provider_name) => workflow_runtime::inspect_provider(
            log,
            pack.name(),
            pack.directory(),
            provider_name.as_deref(),
        )
        .map(|outcome| outcome.map(InspectKnowledgeResult::Provider)),
    };
    match outcome {
        Ok(workflow_runtime::RuntimeOutcome::Success { result, log_path }) => {
            response::prepare_success_with_log(result, Some(log_path))
        }
        Ok(workflow_runtime::RuntimeOutcome::Failure {
            diagnostic,
            log_path,
        }) => response::prepare_runtime_failure(diagnostic, log_path),
        Err(error) => {
            let log_path = error.log_path();
            response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
        }
    }
}

fn finish_inspect_target_failure(
    mut log: OperationLog,
    error: InspectTargetPackError,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let details = format!(
        "status: failure\nerror: {}\n",
        project_inline_text(&error.to_string())
    );
    if let Err(log_error) = log.append(details.as_bytes()) {
        return inspect_target_log_failure(log_error);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(error) => inspect_target_log_failure(error),
    }
}

fn inspect_target_log_failure(
    error: OperationLogError,
) -> response::PreparedResponse<InspectKnowledgeResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        InspectTargetPackError::IncompleteOperationLog(error)
    } else {
        InspectTargetPackError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

fn inspect_packs(pack_directories: Vec<PathBuf>) -> Result<InspectPacksResult, InspectPacksError> {
    let skill_root = locate_skill_root()?;
    let data_home = locate_data_home()?;
    let discovered = pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: pack_directories,
    })
    .map_err(PackDiscoveryFailure::from)?;

    Ok(InspectPacksResult {
        packs: discovered.iter().map(project_pack).collect(),
    })
}

fn locate_data_home() -> Result<PathBuf, configuration::ConfigurationError> {
    configuration::data_home()
}

fn project_pack(pack: &DiscoveredPack) -> PackResult {
    PackResult {
        name: pack.name().to_owned(),
        title: pack.title().to_owned(),
        description: pack.description().to_owned(),
        owner: pack.owner().to_owned(),
    }
}

fn locate_skill_root() -> Result<PathBuf, SkillRootError> {
    let executable = std::env::current_exe().map_err(SkillRootError::CurrentExecutable)?;
    let payload = executable
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let targets = payload
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let scripts = targets
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let skill = scripts
        .parent()
        .ok_or_else(|| SkillRootError::InvalidLayout {
            executable: executable.clone(),
        })?;
    let expected_binary = if cfg!(windows) { "kat.exe" } else { "kat" };
    if executable.file_name().and_then(|name| name.to_str()) != Some(expected_binary)
        || targets.file_name().and_then(|name| name.to_str()) != Some("targets")
        || scripts.file_name().and_then(|name| name.to_str()) != Some("scripts")
    {
        return Err(SkillRootError::InvalidLayout { executable });
    }

    let marker = skill.join("SKILL.md");
    let metadata = fs::symlink_metadata(&marker).map_err(|source| SkillRootError::SkillMarker {
        path: marker.clone(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(SkillRootError::SkillMarkerIsNotFile { path: marker });
    }
    dunce::canonicalize(skill).map_err(|source| SkillRootError::CanonicalSkillRoot {
        path: skill.to_path_buf(),
        source,
    })
}

#[derive(Debug, Error)]
enum SkillRootError {
    #[error("failed to locate the current executable")]
    CurrentExecutable(#[source] io::Error),
    #[error("KAT executable is not in <skill>/scripts/targets/<target>: {executable}")]
    InvalidLayout { executable: PathBuf },
    #[error("failed to inspect KAT Skill marker {path}")]
    SkillMarker {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("KAT Skill marker is not a regular file: {path}")]
    SkillMarkerIsNotFile { path: PathBuf },
    #[error("failed to resolve KAT Skill root {path}")]
    CanonicalSkillRoot {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Error, Diagnostic)]
enum PackDiscoveryFailure {
    #[error("PACK discovery failed")]
    #[diagnostic(help("Correct the first invalid PACK candidate and retry"))]
    Discovery {
        #[source]
        source: Box<pack_discovery::PackDiscoveryError>,
    },
    #[error("PACK discovery failed")]
    #[diagnostic(help(
        "Make the default PACK search path a readable directory or remove it, then retry"
    ))]
    DefaultPackSearchPath {
        #[source]
        source: Box<pack_discovery::PackDiscoveryError>,
    },
    #[error("PACK discovery failed")]
    #[diagnostic(help("Remove one conflicting PACK or give the PACKs distinct names, then retry"))]
    DuplicatePackName {
        #[source]
        source: Box<pack_discovery::PackDiscoveryError>,
    },
}

impl From<pack_discovery::PackDiscoveryError> for PackDiscoveryFailure {
    fn from(source: pack_discovery::PackDiscoveryError) -> Self {
        match source {
            source @ pack_discovery::PackDiscoveryError::DuplicatePackName { .. } => {
                Self::DuplicatePackName {
                    source: Box::new(source),
                }
            }
            source @ pack_discovery::PackDiscoveryError::ReadSearchDirectory { .. }
            | source @ pack_discovery::PackDiscoveryError::EnumerateSearchDirectory { .. }
            | source @ pack_discovery::PackDiscoveryError::InspectSearchEntry { .. } => {
                Self::DefaultPackSearchPath {
                    source: Box::new(source),
                }
            }
            source => Self::Discovery {
                source: Box::new(source),
            },
        }
    }
}

#[derive(Debug, Error, Diagnostic)]
enum InspectPacksError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help(
        "Run kat from <skill>/scripts/targets/<target> with a regular <skill>/SKILL.md marker"
    ))]
    SkillRoot(
        #[from]
        #[source]
        SkillRootError,
    ),
    #[error(transparent)]
    #[diagnostic(transparent)]
    DataHome(#[from] configuration::ConfigurationError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    PackDiscovery(#[from] PackDiscoveryFailure),
}

#[derive(Debug, Error, Diagnostic)]
enum InspectTargetPackError {
    #[error(transparent)]
    #[diagnostic(transparent)]
    SessionStore(#[from] session_store::SessionStoreError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    PublishedRun(#[from] run_manifest::PublishedRunError),
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("PACK inspection Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete inspection"))]
    OperationLog(#[source] OperationLogError),
    #[error("PACK inspection Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(#[source] OperationLogError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    PackDiscovery(#[from] PackDiscoveryFailure),
    #[error("PACK {name:?} was not discovered")]
    #[diagnostic(help(
        "Use the exact manifest name from `kat inspect`, or add its directory with --pack-dir"
    ))]
    UnknownPack { name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_the_separate_inspection_modes() {
        for arguments in [
            vec![
                "kat",
                "inspect",
                "--pack-dir",
                "first",
                "--pack-dir",
                "second",
            ],
            vec![
                "kat",
                "inspect",
                "workflow",
                "--pack",
                "cpu-pack",
                "--pack-dir",
                "checkout",
            ],
            vec![
                "kat",
                "inspect",
                "workflow",
                "--pack",
                "cpu-pack",
                "--workflow",
                "thread-time",
            ],
            vec![
                "kat",
                "inspect",
                "workflow",
                "--session",
                "session-id",
                "--run",
                "run-id",
            ],
            vec!["kat", "inspect", "session", "--session", "session-id"],
            vec!["kat", "inspect", "provider", "--pack", "cpu-pack"],
            vec![
                "kat",
                "inspect",
                "provider",
                "--pack",
                "cpu-pack",
                "--provider",
                "postgresql",
                "--pack-dir",
                "checkout",
            ],
        ] {
            assert!(
                Cli::try_parse_from(&arguments).is_ok(),
                "expected valid arguments: {arguments:?}"
            );
        }
    }

    #[test]
    fn parser_rejects_ambiguous_or_legacy_inspection_modes() {
        for arguments in [
            vec!["kat", "inspect", "--pack", "cpu-pack"],
            vec!["kat", "inspect", "--dataset", "dataset"],
            vec!["kat", "inspect", "workflow"],
            vec!["kat", "inspect", "workflow", "--workflow", "thread-time"],
            vec!["kat", "inspect", "workflow", "--run", "run-id"],
            vec!["kat", "inspect", "workflow", "--session", "session-id"],
            vec![
                "kat", "inspect", "workflow", "--pack", "cpu-pack", "--run", "run-id",
            ],
            vec![
                "kat",
                "inspect",
                "workflow",
                "--run",
                "run-id",
                "--workflow",
                "thread-time",
            ],
            vec!["kat", "inspect", "provider"],
            vec!["kat", "inspect", "provider", "--run", "run-id"],
            vec![
                "kat",
                "inspect",
                "session",
                "--session",
                "session-id",
                "--pack-dir",
                "checkout",
            ],
        ] {
            assert!(
                Cli::try_parse_from(&arguments).is_err(),
                "expected invalid arguments: {arguments:?}"
            );
        }
    }

    #[test]
    fn parser_rejects_bare_and_unknown_operations() {
        assert!(Cli::try_parse_from(["kat"]).is_err());
        assert!(Cli::try_parse_from(["kat", "list"]).is_err());
        assert!(Cli::try_parse_from(["kat", "inspect", "--version"]).is_err());
    }
}
