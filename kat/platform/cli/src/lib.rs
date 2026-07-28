mod operation_log;
mod pack_discovery;
mod query;
mod response;
mod run;
mod test;
mod text_projection;
mod workflow_runtime;

use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};
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
    /// Import one source into a complete KAT Dataset.
    Import(ImportArgs),
    /// Inspect available PACKs, one exact PACK, or one KAT Dataset.
    Inspect {
        /// Inspect one exact PACK by manifest name.
        #[arg(long, value_name = "NAME", conflicts_with = "dataset")]
        pack: Option<String>,
        /// Inspect one managed KAT Dataset and its Parquet Schema.
        #[arg(
            long,
            value_name = "DIRECTORY",
            conflicts_with_all = ["pack", "pack_directories"]
        )]
        dataset: Option<PathBuf>,
        #[arg(
            long = "pack-dir",
            value_name = "DIRECTORY",
            help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order; results remain sorted by PACK name."
        )]
        pack_directories: Vec<PathBuf>,
    },
    /// Execute one Workflow and atomically publish one Run.
    ///
    /// The Operation log may retain the resolved PACK path, optional Dataset
    /// path, and all arguments after `--`. Do not pass secrets in these values.
    Run(run::RunArgs),
    /// Query one published Run's output.* and optional current dataset.* tables.
    ///
    /// Rows are positional JSON scalars. int64/uint64 and Decimal values are
    /// decimal strings. Timestamp(ns, UTC) values are RFC 3339 strings
    /// normalized to UTC with Z. Other supported integers and finite floats are
    /// JSON numbers; bool, string, and null retain their JSON kinds.
    ///
    /// The Operation log retains the complete --sql value. Do not pass secrets
    /// in it.
    Query(query::QueryArgs),
    /// Run one PACK's pytest suite in the production execution plane.
    Test(test::TestArgs),
}

#[derive(Args)]
struct ImportArgs {
    /// Write the Dataset at this exact directory.
    #[arg(long, value_name = "DIRECTORY", global = true)]
    dataset: Option<PathBuf>,
    /// Replace the Dataset at the resolved target path. Permanently deletes all existing contents, including unrecognized files. Linked or mounted paths may affect data outside the path you typed. No backup, rollback, or failure recovery is provided.
    #[arg(long, global = true, requires = "dataset")]
    overwrite_dataset: bool,
    #[command(subcommand)]
    datasource: Datasource,
}

#[derive(Subcommand)]
enum Datasource {
    /// Import a HiProfiler Hitrace capture as normalized long-term source facts.
    Hitrace {
        /// Read the Hitrace capture at this path.
        #[arg(long, value_name = "PATH")]
        trace: PathBuf,
    },
    /// Deprecated: pre-release validation only. Its table interface is unstable and it must be removed before the first formal release.
    TraceStreamer {
        /// Read the Trace Streamer SQLite database at this path.
        #[arg(long, value_name = "PATH")]
        database: PathBuf,
    },
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
    required_tables: Vec<String>,
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
        Operation::Import(ImportArgs {
            dataset,
            overwrite_dataset,
            datasource: Datasource::Hitrace { trace },
        }) => response::publish(import_hitrace(trace, dataset, overwrite_dataset)),
        Operation::Import(ImportArgs {
            dataset,
            overwrite_dataset,
            datasource: Datasource::TraceStreamer { database },
        }) => {
            let prepared = match import_trace_streamer(database, dataset, overwrite_dataset) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
        Operation::Inspect {
            dataset: Some(dataset),
            ..
        } => {
            let prepared = match inspect_dataset(dataset) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
        Operation::Inspect {
            dataset: None,
            pack: Some(pack),
            pack_directories,
        } => response::publish(inspect_target_pack(pack, pack_directories)),
        Operation::Inspect {
            dataset: None,
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
    let Some(data_home) = locate_data_home() else {
        return response::prepare_cli_failure(miette::Report::new(
            InspectTargetPackError::DataHomeUnavailable,
        ));
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
                required_tables: workflow.required_tables,
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

#[derive(Serialize)]
struct ImportHitraceResult {
    path: String,
    unsupported_plugins: Vec<String>,
    unsupported_section_types: Vec<u32>,
}

fn import_hitrace(
    trace: PathBuf,
    dataset: Option<PathBuf>,
    overwrite: bool,
) -> response::PreparedResponse<ImportHitraceResult> {
    let Some(data_home) = locate_data_home() else {
        return response::prepare_cli_failure(miette::Report::new(
            ImportHitraceError::DataHomeUnavailable,
        ));
    };
    let target = dataset.unwrap_or_else(|| {
        data_home
            .join("datasets")
            .join(uuid::Uuid::now_v7().to_string())
    });
    let mut log = match OperationLog::create(&data_home, "import", |file| {
        writeln!(
            file,
            "operation: kat import hitrace\ntrace: {trace:?}\ndataset: {target:?}"
        )
    }) {
        Ok(log) => log,
        Err(error) => return operation_log_failure(error),
    };
    if let Err(source) = locate_skill_root() {
        let error = ImportHitraceError::SkillRoot(source);
        if let Err(source) = writeln!(log, "status: failure\nerror: {error}") {
            return finish_hitrace_failure(log, ImportHitraceError::WriteOperationLog { source });
        }
        return finish_hitrace_failure(log, error);
    }
    let target = if overwrite {
        kat_datasource::DatasetWriteTarget::permanently_replace_all_contents(target)
    } else {
        kat_datasource::DatasetWriteTarget::write_to_empty(target)
    }
    .protect_path(log.path());
    let imported = match kat_datasource::import_hitrace(&trace, target, |content| {
        write_unsupported_hitrace_content(&mut log, content)
    }) {
        Ok(imported) => imported,
        Err(kat_datasource::HitraceImportError::ObserveUnsupportedContent { source }) => {
            return finish_hitrace_failure(log, ImportHitraceError::WriteOperationLog { source });
        }
        Err(source) => {
            let error = ImportHitraceError::Import { source };
            if let Err(source) = writeln!(log, "status: failure\nerror: {error}") {
                return finish_hitrace_failure(
                    log,
                    ImportHitraceError::WriteOperationLog { source },
                );
            }
            return finish_hitrace_failure(log, error);
        }
    };
    let path = match imported.path().to_str() {
        Some(path) => path.to_owned(),
        None => {
            let error = ImportHitraceError::NonUnicodeDataset {
                path: imported.path().to_path_buf(),
            };
            if let Err(source) = writeln!(log, "status: failure\nerror: {error}") {
                return finish_hitrace_failure(
                    log,
                    ImportHitraceError::WriteOperationLog { source },
                );
            }
            return finish_hitrace_failure(log, error);
        }
    };
    if let Err(source) = writeln!(log, "status: success") {
        return finish_hitrace_failure(log, ImportHitraceError::WriteOperationLog { source });
    }
    let result = ImportHitraceResult {
        path,
        unsupported_plugins: imported.unsupported_plugins().to_vec(),
        unsupported_section_types: imported.unsupported_section_types().to_vec(),
    };
    match log.finish() {
        Ok(log_path) => response::prepare_success_with_log(result, Some(log_path)),
        Err(error) => operation_log_failure(error),
    }
}

fn write_unsupported_hitrace_content(
    log: &mut dyn Write,
    unsupported: &kat_datasource::UnsupportedHitraceContent,
) -> io::Result<()> {
    writeln!(
        log,
        "unsupported {} {:?} at byte {}",
        unsupported.kind(),
        unsupported.value(),
        unsupported.byte_offset()
    )
}

fn finish_hitrace_failure(
    log: OperationLog,
    error: ImportHitraceError,
) -> response::PreparedResponse<ImportHitraceResult> {
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(log_error) => operation_log_failure(log_error),
    }
}

fn operation_log_failure(
    error: OperationLogError,
) -> response::PreparedResponse<ImportHitraceResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        ImportHitraceError::IncompleteOperationLog(error)
    } else {
        ImportHitraceError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

#[derive(Debug, Error, Diagnostic)]
enum ImportHitraceError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on a supported platform with a standard user data directory"))]
    DataHomeUnavailable,
    #[error("Hitrace Import Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete Import"))]
    OperationLog(#[source] OperationLogError),
    #[error("Hitrace Import Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(#[source] OperationLogError),
    #[error("Hitrace Import failed")]
    #[diagnostic(help("Correct the capture or Dataset target and retry the complete Import"))]
    Import {
        #[source]
        source: kat_datasource::HitraceImportError,
    },
    #[error("Hitrace Import Operation log is incomplete because a write failed")]
    WriteOperationLog {
        #[source]
        source: io::Error,
    },
    #[error("Dataset path cannot be represented as native Unicode: {path:?}")]
    NonUnicodeDataset { path: PathBuf },
}

#[derive(Serialize)]
struct ImportTraceStreamerResult {
    path: String,
}

fn import_trace_streamer(
    database: PathBuf,
    dataset: Option<PathBuf>,
    overwrite: bool,
) -> Result<ImportTraceStreamerResult, ImportTraceStreamerError> {
    locate_skill_root().map_err(ImportTraceStreamerError::SkillRoot)?;
    let database = dunce::canonicalize(&database).map_err(|source| {
        ImportTraceStreamerError::CanonicalDatabase {
            path: database,
            source,
        }
    })?;
    if database.to_str().is_none() {
        return Err(ImportTraceStreamerError::NonUnicodeDatabase { path: database });
    }
    let target = match dataset {
        Some(path) => path,
        None => locate_data_home()
            .ok_or(ImportTraceStreamerError::DataHomeUnavailable)?
            .join("datasets")
            .join(uuid::Uuid::now_v7().to_string()),
    };
    let target = if overwrite {
        kat_datasource::DatasetWriteTarget::permanently_replace_all_contents(target)
    } else {
        kat_datasource::DatasetWriteTarget::write_to_empty(target)
    };
    let imported = kat_datasource::import_deprecated_trace_streamer(&database, target)
        .map_err(|source| ImportTraceStreamerError::Import { source })?;
    let path = imported
        .path()
        .to_str()
        .ok_or_else(|| ImportTraceStreamerError::NonUnicodeDataset {
            path: imported.path().to_path_buf(),
        })?
        .to_owned();
    Ok(ImportTraceStreamerResult { path })
}

fn inspect_packs(pack_directories: Vec<PathBuf>) -> Result<InspectPacksResult, InspectPacksError> {
    let skill_root = locate_skill_root()?;
    let data_home = locate_data_home().ok_or(InspectPacksError::DataHomeUnavailable)?;
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

fn locate_data_home() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "KAT")
        .map(|project_dirs| project_dirs.data_dir().to_path_buf())
}

fn project_pack(pack: &DiscoveredPack) -> PackResult {
    PackResult {
        name: pack.name().to_owned(),
        title: pack.title().to_owned(),
        description: pack.description().to_owned(),
        owner: pack.owner().to_owned(),
    }
}

#[derive(Serialize)]
struct InspectDatasetResult {
    path: String,
    tables: Vec<DatasetTableResult>,
}

#[derive(Serialize)]
struct DatasetTableResult {
    name: String,
    columns: Vec<DatasetColumnResult>,
}

#[derive(Serialize)]
struct DatasetColumnResult {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
    nullable: bool,
}

fn inspect_dataset(path: PathBuf) -> Result<InspectDatasetResult, InspectDatasetError> {
    let inspection = kat_datasource::inspect_dataset(&path)
        .map_err(|source| InspectDatasetError::Inspection { source })?;
    let canonical_path = inspection
        .path()
        .to_str()
        .ok_or_else(|| InspectDatasetError::NonUnicodePath {
            path: inspection.path().to_path_buf(),
        })?
        .to_owned();
    Ok(InspectDatasetResult {
        path: canonical_path,
        tables: inspection
            .tables()
            .iter()
            .map(|table| DatasetTableResult {
                name: table.name().to_owned(),
                columns: table
                    .columns()
                    .iter()
                    .map(|column| DatasetColumnResult {
                        name: column.name().to_owned(),
                        data_type: column.data_type().to_owned(),
                        nullable: column.nullable(),
                    })
                    .collect(),
            })
            .collect(),
    })
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
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on Linux or Windows with a platform standard user data directory"))]
    DataHomeUnavailable,
    #[error(transparent)]
    #[diagnostic(transparent)]
    PackDiscovery(#[from] PackDiscoveryFailure),
}

#[derive(Debug, Error, Diagnostic)]
enum InspectTargetPackError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on a supported platform with a standard user data directory"))]
    DataHomeUnavailable,
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

#[derive(Debug, Error, Diagnostic)]
enum InspectDatasetError {
    #[error("Dataset inspection failed")]
    #[diagnostic(help("Provide a complete KAT Dataset directory and retry"))]
    Inspection {
        #[source]
        source: kat_datasource::DatasetInspectionError,
    },
    #[error("Dataset path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { path: PathBuf },
}

#[derive(Debug, Error, Diagnostic)]
enum ImportTraceStreamerError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Use --dataset with an explicit target on a supported local filesystem"))]
    DataHomeUnavailable,
    #[error("failed to resolve Trace Streamer database {path}")]
    #[diagnostic(help("Provide an existing readable Trace Streamer SQLite database"))]
    CanonicalDatabase {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Trace Streamer database path cannot be represented as native Unicode: {path:?}")]
    NonUnicodeDatabase { path: PathBuf },
    #[error("Trace Streamer Import failed")]
    #[diagnostic(help(
        "Correct the source database or Dataset target and retry the complete Import"
    ))]
    Import {
        #[source]
        source: kat_datasource::TraceStreamerImportError,
    },
    #[error("Dataset path cannot be represented as native Unicode: {path:?}")]
    NonUnicodeDataset { path: PathBuf },
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
            dataset,
            pack,
            pack_directories,
        } = cli.operation
        else {
            panic!("expected inspect operation");
        };
        assert!(dataset.is_none());
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
        let Operation::Inspect { pack, dataset, .. } = cli.operation else {
            panic!("expected inspect operation");
        };
        assert_eq!(pack.as_deref(), Some("cpu-pack"));
        assert!(dataset.is_none());

        assert!(
            Cli::try_parse_from([
                "kat",
                "inspect",
                "--pack",
                "cpu-pack",
                "--dataset",
                "dataset",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "kat",
                "inspect",
                "--dataset",
                "dataset",
                "--pack-dir",
                "checkout",
            ])
            .is_err()
        );
    }

    #[test]
    fn parser_rejects_bare_and_unknown_operations() {
        assert!(Cli::try_parse_from(["kat"]).is_err());
        assert!(Cli::try_parse_from(["kat", "list"]).is_err());
        assert!(Cli::try_parse_from(["kat", "inspect", "--version"]).is_err());
    }

    #[test]
    fn parser_accepts_import_target_options_on_both_sides_of_datasource() {
        for arguments in [
            vec![
                "kat",
                "import",
                "--dataset",
                "target",
                "--overwrite-dataset",
                "trace-streamer",
                "--database",
                "source.db",
            ],
            vec![
                "kat",
                "import",
                "trace-streamer",
                "--database",
                "source.db",
                "--dataset",
                "target",
                "--overwrite-dataset",
            ],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
        assert!(
            Cli::try_parse_from([
                "kat",
                "import",
                "hitrace",
                "--trace",
                "capture.htrace",
                "--dataset",
                "target",
            ])
            .is_ok()
        );
    }

    #[test]
    fn parser_rejects_overwrite_without_explicit_dataset() {
        assert!(
            Cli::try_parse_from([
                "kat",
                "import",
                "trace-streamer",
                "--database",
                "source.db",
                "--overwrite-dataset",
            ])
            .is_err()
        );
    }
}
