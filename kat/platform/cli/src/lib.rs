mod configuration;
mod operation_log;
mod pack_discovery;
mod query;
mod response;
mod run;
mod test;
mod text_projection;
mod workflow_runtime;

use std::{fs, io, path::PathBuf, process::ExitCode};

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
    /// Inspect available PACKs or one exact PACK.
    Inspect {
        /// Inspect one exact PACK by manifest name.
        #[arg(long, value_name = "NAME")]
        pack: Option<String>,
        #[arg(
            long = "pack-dir",
            value_name = "DIRECTORY",
            help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order; results remain sorted by PACK name."
        )]
        pack_directories: Vec<PathBuf>,
    },
    /// Execute one Workflow and atomically publish one Run.
    ///
    /// The Operation log may retain the resolved PACK path and all arguments
    /// after `--`. Do not pass secrets in these values.
    Run(run::RunArgs),
    /// Query one published Run's output.* tables.
    ///
    /// DataFusion writes Arrow's native object-row JSON mapping directly to one
    /// NDJSON result file. A successful response contains that path and the
    /// ordered result columns; the CLI does not read or re-encode query rows.
    ///
    /// The Operation log retains the complete --sql value. Do not pass secrets
    /// in it.
    Query(query::QueryArgs),
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
struct InspectPackResult {
    name: String,
    title: String,
    description: String,
    owner: String,
    workflows: Vec<InspectWorkflowResult>,
}

#[derive(Serialize)]
struct InspectWorkflowResult {
    name: String,
    title: String,
    description: String,
    parameters: Vec<InspectParameterResult>,
}

#[derive(Serialize)]
struct InspectParameterResult {
    name: String,
    option: String,
    #[serde(rename = "type")]
    parameter_type: workflow_runtime::ParameterType,
    required: bool,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    negative_option: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<String>>,
    #[serde(skip_serializing_if = "workflow_runtime::ParameterDefault::is_missing")]
    default: workflow_runtime::ParameterDefault,
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
        Operation::Inspect {
            pack: Some(pack),
            pack_directories,
        } => response::publish(inspect_target_pack(pack, pack_directories)),
        Operation::Inspect {
            pack: None,
            pack_directories,
        } => {
            let prepared = match inspect_packs(pack_directories) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
        Operation::Run(arguments) => response::publish(run::execute(arguments)),
        Operation::Query(arguments) => response::publish(query::execute(arguments)),
        Operation::Test(arguments) => response::publish(test::execute(arguments)),
    }
}

fn inspect_target_pack(
    pack_name: String,
    pack_directories: Vec<PathBuf>,
) -> response::PreparedResponse<InspectPackResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let mut log = match OperationLog::create(&data_home, "inspect", |file| {
        writeln!(
            file,
            "operation: kat inspect --pack\npack: {}",
            pack_name.escape_debug()
        )
    }) {
        Ok(log) => log,
        Err(error) => return inspect_target_log_failure(error),
    };
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

    match workflow_runtime::inspect_pack(log, pack.name(), pack.directory()) {
        Ok(workflow_runtime::InspectPackOutcome::Success { result, log_path }) => {
            response::prepare_success_with_log(project_inspected_pack(pack, result), Some(log_path))
        }
        Ok(workflow_runtime::InspectPackOutcome::Failure {
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
) -> response::PreparedResponse<InspectPackResult> {
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
) -> response::PreparedResponse<InspectPackResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        InspectTargetPackError::IncompleteOperationLog(error)
    } else {
        InspectTargetPackError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

fn project_inspected_pack(
    pack: &DiscoveredPack,
    workflows: Vec<workflow_runtime::Workflow>,
) -> InspectPackResult {
    InspectPackResult {
        name: pack.name().to_owned(),
        title: pack.title().to_owned(),
        description: pack.description().to_owned(),
        owner: pack.owner().to_owned(),
        workflows: workflows
            .into_iter()
            .map(|workflow| InspectWorkflowResult {
                name: workflow.name,
                title: workflow.title,
                description: workflow.description,
                parameters: workflow
                    .parameters
                    .into_iter()
                    .map(|parameter| InspectParameterResult {
                        name: parameter.name,
                        option: parameter.option,
                        parameter_type: parameter.parameter_type,
                        required: parameter.required,
                        description: parameter.description,
                        negative_option: parameter.negative_option,
                        choices: parameter.choices,
                        default: parameter.default,
                    })
                    .collect(),
            })
            .collect(),
    }
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
    fn parser_accepts_ordered_repeated_pack_directories() {
        let cli = Cli::try_parse_from([
            "kat",
            "inspect",
            "--pack-dir",
            "first",
            "--pack-dir",
            "second",
        ])
        .expect("parse inspect");

        let Operation::Inspect {
            pack,
            pack_directories,
        } = cli.operation
        else {
            panic!("expected inspect operation");
        };
        assert!(pack.is_none());
        assert_eq!(
            pack_directories,
            [PathBuf::from("first"), PathBuf::from("second")]
        );
    }

    #[test]
    fn parser_accepts_one_exact_pack_target_and_rejects_other_inspect_modes() {
        let cli = Cli::try_parse_from([
            "kat",
            "inspect",
            "--pack",
            "cpu-pack",
            "--pack-dir",
            "checkout",
        ])
        .expect("parse targeted PACK inspection");
        let Operation::Inspect { pack, .. } = cli.operation else {
            panic!("expected inspect operation");
        };
        assert_eq!(pack.as_deref(), Some("cpu-pack"));
        assert!(Cli::try_parse_from(["kat", "inspect", "--dataset", "dataset",]).is_err());
    }

    #[test]
    fn parser_rejects_bare_and_unknown_operations() {
        assert!(Cli::try_parse_from(["kat"]).is_err());
        assert!(Cli::try_parse_from(["kat", "list"]).is_err());
        assert!(Cli::try_parse_from(["kat", "inspect", "--version"]).is_err());
    }
}
