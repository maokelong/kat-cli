mod operation_log;
mod pack_discovery;
mod response;
mod text_projection;
mod workflow_runtime;

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};
use miette::Diagnostic;
use operation_log::{OperationLog, OperationLogError};
use pack_discovery::{DiscoveredPack, PackDiscoveryPaths};
use serde::{Deserialize, Serialize};
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
    /// Inspect the PACKs available to this KAT Skill.
    Inspect {
        /// Inspect one exact PACK by manifest name.
        #[arg(long, value_name = "NAME", conflicts_with = "dataset")]
        pack: Option<String>,
        /// Inspect one KAT Dataset directory.
        #[arg(
            long,
            value_name = "DIRECTORY",
            conflicts_with_all = ["pack", "pack_directories"]
        )]
        dataset: Option<PathBuf>,
        #[arg(
            long = "pack-dir",
            value_name = "DIRECTORY",
            help = "Add a PACK directory for this command. The directory must directly contain pack.toml. Repeat to add more PACKs."
        )]
        pack_directories: Vec<PathBuf>,
    },
    /// Execute one Workflow and atomically publish one Run.
    Run(RunArgs),
    /// Query the Table Outputs of one published Run.
    Query(QueryArgs),
}

#[derive(Args)]
struct QueryArgs {
    /// Select one exact published Run ID.
    #[arg(long, value_name = "RUN_ID")]
    run: String,
    /// Execute one unmodified read-only DataFusion SQL statement.
    #[arg(long, value_name = "SQL")]
    sql: String,
}

#[derive(Args)]
struct RunArgs {
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
        help = "Add a PACK directory for this command. Repeat to add more PACKs."
    )]
    pack_directories: Vec<PathBuf>,
    /// Forward all tokens after `--` unchanged to the Workflow Input Compiler.
    #[arg(last = true, value_name = "ARGUMENT")]
    workflow_arguments: Vec<String>,
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
    parameter_type: String,
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
        Operation::Run(arguments) => response::publish(run_workflow(arguments)),
        Operation::Query(arguments) => response::publish(query_run(arguments)),
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
        writeln!(file, "operation: kat inspect --pack\npack: {pack_name}")
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
                InspectTargetPackError::Discovery { source },
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
        Ok(workflow_runtime::InspectPackOutcome::Success {
            workflows,
            log_path,
        }) => response::prepare_success_with_log(
            project_inspected_pack(pack, workflows),
            Some(log_path),
        ),
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
    if let Err(log_error) = log.append(format!("status: failure\nerror: {error}\n").as_bytes()) {
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
struct RunResult {
    run_id: String,
    outputs: BTreeMap<String, PublicOutput>,
}

#[derive(Serialize)]
struct PublicOutput {
    columns: Vec<workflow_runtime::Column>,
    row_count: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RunManifest {
    run_id: String,
    pack: String,
    workflow: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dataset: Option<String>,
    inputs: BTreeMap<String, serde_json::Value>,
    outputs: BTreeMap<String, workflow_runtime::RuntimeOutput>,
}

const QUERY_RESPONSE_BYTE_LIMIT: usize = 256 * 1024;

#[derive(Serialize)]
struct QueryResult {
    dataset: QueryDatasetResult,
    columns: Vec<workflow_runtime::Column>,
    rows: Vec<Vec<serde_json::Value>>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum QueryDatasetResult {
    NotProvided,
    Available { path: String },
    Unavailable { path: String, cause: String },
}

impl RunManifest {
    fn new(
        candidate_id: String,
        pack: String,
        workflow: String,
        dataset: Option<String>,
        runtime: workflow_runtime::RunWorkflowResult,
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

fn run_workflow(arguments: RunArgs) -> response::PreparedResponse<RunResult> {
    let Some(data_home) = locate_data_home() else {
        return response::prepare_cli_failure(miette::Report::new(
            RunOperationError::DataHomeUnavailable,
        ));
    };
    let candidate_id = uuid::Uuid::now_v7().to_string();
    let mut log = match OperationLog::create_run(&data_home, &candidate_id, |file| {
        writeln!(
            file,
            "operation: kat run\ncandidate: {candidate_id}\npack: {}\nworkflow: {}",
            arguments.pack, arguments.workflow
        )
    }) {
        Ok(log) => log,
        Err(error) => return run_log_failure(error),
    };
    let candidate = match create_run_candidate(&data_home, &candidate_id) {
        Ok(path) => path,
        Err(error) => return finish_run_failure(log, error),
    };
    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => {
            return finish_run_failure(log, RunOperationError::SkillRoot(source));
        }
    };
    let discovered = match pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: arguments.pack_directories,
    }) {
        Ok(discovered) => discovered,
        Err(source) => {
            return finish_run_failure(log, RunOperationError::Discovery { source });
        }
    };
    let Some(pack) = discovered.get(&arguments.pack) else {
        return finish_run_failure(
            log,
            RunOperationError::UnknownPack {
                name: arguments.pack,
            },
        );
    };
    let dataset = match arguments.dataset {
        Some(path) => match kat_datasource::resolve_dataset(&path) {
            Ok(dataset) => Some(dataset),
            Err(source) => {
                return finish_run_failure(log, RunOperationError::Dataset { source });
            }
        },
        None => None,
    };
    let runtime_dataset = match dataset.as_ref().map(project_resolved_dataset).transpose() {
        Ok(dataset) => dataset,
        Err(error) => return finish_run_failure(log, error),
    };
    let dataset_path = runtime_dataset.as_ref().map(|dataset| dataset.path.clone());
    let Some(pack_path) = pack.directory().to_str().map(str::to_owned) else {
        return finish_run_failure(
            log,
            RunOperationError::NonUnicodePath {
                label: "PACK",
                path: pack.directory().to_path_buf(),
            },
        );
    };
    let Some(run_path) = candidate.to_str().map(str::to_owned) else {
        return finish_run_failure(log, RunOperationError::PrivateCandidatePath);
    };
    if let Err(error) = log.append(
        format!(
            "pack_path: {:?}\ndataset: {}\narguments: {:?}\n",
            pack.directory(),
            dataset_path.as_deref().unwrap_or("not provided"),
            arguments.workflow_arguments
        )
        .as_bytes(),
    ) {
        return run_log_failure(error);
    }

    let outcome = workflow_runtime::run_workflow(
        log,
        workflow_runtime::RunWorkflowInvocation {
            pack_name: pack.name().to_owned(),
            pack_path,
            workflow_name: arguments.workflow.clone(),
            dataset: runtime_dataset,
            arguments: arguments.workflow_arguments,
            candidate_id: candidate_id.clone(),
            run_path,
        },
    );
    let (runtime, log_path) = match outcome {
        Ok(workflow_runtime::RunWorkflowOutcome::Success { result, log_path }) => {
            (result, log_path)
        }
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
    if let Err(error) = publish_run_manifest(&candidate, &manifest) {
        return response::prepare_cli_failure_with_log(miette::Report::new(error), Some(log_path));
    }
    response::prepare_success_with_log(result, Some(log_path))
}

fn query_run(arguments: QueryArgs) -> response::PreparedResponse<QueryResult> {
    let Some(data_home) = locate_data_home() else {
        return response::prepare_cli_failure(miette::Report::new(
            QueryOperationError::DataHomeUnavailable,
        ));
    };
    let mut log = match OperationLog::create(&data_home, "query", |file| {
        writeln!(
            file,
            "operation: kat query\nrun: {:?}\nsql: {:?}",
            arguments.run, arguments.sql
        )
    }) {
        Ok(log) => log,
        Err(error) => return query_log_failure(error),
    };
    if let Err(source) = locate_skill_root() {
        return finish_query_failure(log, QueryOperationError::SkillRoot(source));
    }
    let (run_path, manifest) = match read_run_manifest(&data_home, &arguments.run) {
        Ok(value) => value,
        Err(error) => return finish_query_failure(log, error),
    };
    let (runtime_dataset, public_dataset) = match resolve_query_dataset(manifest.dataset.as_deref())
    {
        Ok(value) => value,
        Err(error) => return finish_query_failure(log, error),
    };
    let Some(run_path_text) = run_path.to_str().map(str::to_owned) else {
        return finish_query_failure(log, QueryOperationError::NonUnicodeRunPath);
    };
    let outputs = manifest
        .outputs
        .iter()
        .map(|(name, output)| (name.clone(), output.output_id.clone()))
        .collect();
    if let Err(error) = log.append(
        format!(
            "run_path: {:?}\ndataset_status: {}\noutputs: {}\n",
            run_path,
            query_dataset_status(&public_dataset),
            manifest.outputs.len()
        )
        .as_bytes(),
    ) {
        return query_log_failure(error);
    }
    let outcome = workflow_runtime::query_run(
        log,
        workflow_runtime::QueryRunInvocation {
            run_id: arguments.run,
            run_path: run_path_text,
            outputs,
            dataset: runtime_dataset,
            sql: arguments.sql,
        },
    );
    let (runtime, log_path) = match outcome {
        Ok(workflow_runtime::QueryRunOutcome::Success { result, log_path }) => (result, log_path),
        Ok(workflow_runtime::QueryRunOutcome::Failure {
            diagnostic,
            log_path,
        }) => return response::prepare_runtime_failure(diagnostic, log_path),
        Err(error) => {
            let log_path = error.log_path();
            return response::prepare_cli_failure_with_log(miette::Report::new(error), log_path);
        }
    };
    let result = QueryResult {
        dataset: public_dataset,
        columns: runtime.columns,
        rows: runtime.rows,
    };
    let candidate_size = match response::success_response_size(&result, Some(&log_path)) {
        Ok(size) => size,
        Err(source) => {
            return response::prepare_cli_failure_with_log(
                miette::Report::new(QueryOperationError::EncodeCandidate(source)),
                Some(log_path),
            );
        }
    };
    if candidate_size > QUERY_RESPONSE_BYTE_LIMIT {
        return response::prepare_cli_failure_with_log(
            miette::Report::new(QueryOperationError::ResponseLimit {
                actual: candidate_size,
                limit: QUERY_RESPONSE_BYTE_LIMIT,
            }),
            Some(log_path),
        );
    }
    response::prepare_success_with_log(result, Some(log_path))
}

fn read_run_manifest(
    data_home: &std::path::Path,
    run_id: &str,
) -> Result<(PathBuf, RunManifest), QueryOperationError> {
    let identity = uuid::Uuid::parse_str(run_id)
        .ok()
        .filter(|identity| identity.get_version_num() == 7 && identity.to_string() == run_id)
        .ok_or_else(|| QueryOperationError::RunNotFound {
            run_id: diagnostic_safe_argument(run_id),
        })?;
    debug_assert_eq!(identity.to_string(), run_id);
    let runs = data_home.join("runs");
    let candidate = runs.join(run_id);
    let manifest_path = candidate.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(QueryOperationError::RunNotFound {
            run_id: run_id.to_owned(),
        });
    }
    if manifest_path.is_symlink() {
        return Err(QueryOperationError::InvalidRunLayout);
    }
    let run_path = dunce::canonicalize(&candidate).map_err(QueryOperationError::CorruptRunPath)?;
    let runs_path = dunce::canonicalize(&runs).map_err(QueryOperationError::CorruptRunPath)?;
    if run_path.parent() != Some(runs_path.as_path())
        || run_path.file_name().and_then(|name| name.to_str()) != Some(run_id)
        || !run_path.is_dir()
    {
        return Err(QueryOperationError::InvalidRunLayout);
    }
    let bytes =
        fs::read(run_path.join("manifest.json")).map_err(QueryOperationError::ReadManifest)?;
    let manifest: RunManifest =
        serde_json::from_slice(&bytes).map_err(QueryOperationError::DecodeManifest)?;
    validate_run_manifest(&manifest, run_id)?;
    Ok((run_path, manifest))
}

fn diagnostic_safe_argument(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn validate_run_manifest(manifest: &RunManifest, run_id: &str) -> Result<(), QueryOperationError> {
    if manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
        || manifest
            .dataset
            .as_ref()
            .is_some_and(|path| path.is_empty() || !std::path::Path::new(path).is_absolute())
    {
        return Err(QueryOperationError::InvalidManifestFacts);
    }
    let mut output_ids = std::collections::HashSet::new();
    for (name, output) in &manifest.outputs {
        if !workflow_runtime::valid_output_name(name)
            || output.output_id.len() != 32
            || !output
                .output_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !output_ids.insert(&output.output_id)
            || output
                .columns
                .iter()
                .any(|column| column.name.is_empty() || column.data_type.trim().is_empty())
        {
            return Err(QueryOperationError::InvalidManifestFacts);
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
        return Err(QueryOperationError::InvalidManifestFacts);
    }
    Ok(())
}

fn resolve_query_dataset(
    recorded_path: Option<&str>,
) -> Result<(workflow_runtime::QueryDatasetRequest, QueryDatasetResult), QueryOperationError> {
    let Some(recorded_path) = recorded_path else {
        return Ok((
            workflow_runtime::QueryDatasetRequest::NotProvided,
            QueryDatasetResult::NotProvided,
        ));
    };
    match kat_datasource::resolve_dataset(std::path::Path::new(recorded_path)) {
        Ok(dataset) => {
            let resolved = project_query_dataset(&dataset)?;
            let path = resolved.path.clone();
            Ok((
                workflow_runtime::QueryDatasetRequest::Available {
                    path: resolved.path,
                    tables: resolved.tables,
                },
                QueryDatasetResult::Available { path },
            ))
        }
        Err(source) => {
            let cause = error_chain(&source);
            Ok((
                workflow_runtime::QueryDatasetRequest::Unavailable {
                    path: recorded_path.to_owned(),
                    cause: cause.clone(),
                },
                QueryDatasetResult::Unavailable {
                    path: recorded_path.to_owned(),
                    cause,
                },
            ))
        }
    }
}

fn project_query_dataset(
    dataset: &kat_datasource::ResolvedDataset,
) -> Result<workflow_runtime::ResolvedDatasetRequest, QueryOperationError> {
    let path = query_unicode_path("Dataset", dataset.path())?;
    let mut tables = BTreeMap::new();
    for table in dataset.tables() {
        tables.insert(
            table.name().to_owned(),
            query_unicode_path("Dataset table", table.path())?,
        );
    }
    Ok(workflow_runtime::ResolvedDatasetRequest { path, tables })
}

fn query_unicode_path(
    label: &'static str,
    path: &std::path::Path,
) -> Result<String, QueryOperationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| QueryOperationError::NonUnicodeDatasetPath {
            label,
            path: path.to_path_buf(),
        })
}

fn query_dataset_status(dataset: &QueryDatasetResult) -> &'static str {
    match dataset {
        QueryDatasetResult::NotProvided => "not_provided",
        QueryDatasetResult::Available { .. } => "available",
        QueryDatasetResult::Unavailable { .. } => "unavailable",
    }
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        if !cause.to_string().trim().is_empty() {
            rendered.push_str(": ");
            rendered.push_str(&cause.to_string());
        }
        source = cause.source();
    }
    rendered
}

fn finish_query_failure(
    mut log: OperationLog,
    error: QueryOperationError,
) -> response::PreparedResponse<QueryResult> {
    if let Err(log_error) = log.append(format!("status: failure\nerror: {error:?}\n").as_bytes()) {
        return query_log_failure(log_error);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(error) => query_log_failure(error),
    }
}

fn query_log_failure(error: OperationLogError) -> response::PreparedResponse<QueryResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        QueryOperationError::IncompleteOperationLog(error)
    } else {
        QueryOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

fn create_run_candidate(
    data_home: &std::path::Path,
    id: &str,
) -> Result<PathBuf, RunOperationError> {
    let runs = data_home.join("runs");
    fs::create_dir_all(&runs).map_err(|source| RunOperationError::CreateRuns {
        path: runs.clone(),
        source,
    })?;
    let candidate = runs.join(id);
    fs::create_dir(&candidate).map_err(|source| RunOperationError::CreateCandidate {
        path: candidate.clone(),
        source,
    })?;
    dunce::canonicalize(&candidate).map_err(|source| RunOperationError::CanonicalCandidate {
        path: candidate,
        source,
    })
}

fn project_resolved_dataset(
    dataset: &kat_datasource::ResolvedDataset,
) -> Result<workflow_runtime::ResolvedDatasetRequest, RunOperationError> {
    let path = unicode_path("Dataset", dataset.path())?;
    let mut tables = BTreeMap::new();
    for table in dataset.tables() {
        tables.insert(
            table.name().to_owned(),
            unicode_path("Dataset table", table.path())?,
        );
    }
    Ok(workflow_runtime::ResolvedDatasetRequest { path, tables })
}

fn unicode_path(label: &'static str, path: &std::path::Path) -> Result<String, RunOperationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| RunOperationError::NonUnicodePath {
            label,
            path: path.to_path_buf(),
        })
}

fn publish_run_manifest(
    candidate: &std::path::Path,
    manifest: &RunManifest,
) -> Result<(), RunOperationError> {
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

fn finish_run_failure(
    mut log: OperationLog,
    error: RunOperationError,
) -> response::PreparedResponse<RunResult> {
    if let Err(log_error) = log.append(format!("status: failure\nerror: {error}\n").as_bytes()) {
        return run_log_failure(log_error);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(error) => run_log_failure(error),
    }
}

fn run_log_failure(error: OperationLogError) -> response::PreparedResponse<RunResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        RunOperationError::IncompleteOperationLog(error)
    } else {
        RunOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
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
    let imported = match kat_datasource::import_hitrace(
        &trace,
        kat_datasource::DatasetWriteTarget::new(target, overwrite),
    ) {
        Ok(imported) => imported,
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
    for unsupported in imported.unsupported_content() {
        if let Err(source) = writeln!(
            log,
            "unsupported {} {:?} at byte {}",
            unsupported.kind(),
            unsupported.value(),
            unsupported.byte_offset()
        ) {
            return finish_hitrace_failure(log, ImportHitraceError::WriteOperationLog { source });
        }
    }
    if let Err(source) = writeln!(log, "status: success") {
        return finish_hitrace_failure(log, ImportHitraceError::WriteOperationLog { source });
    }
    let path = match imported.path().to_str() {
        Some(path) => path.to_owned(),
        None => {
            return finish_hitrace_failure(
                log,
                ImportHitraceError::NonUnicodeDataset {
                    path: imported.path().to_path_buf(),
                },
            );
        }
    };
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
    let imported = kat_datasource::import_trace_streamer(
        &database,
        kat_datasource::DatasetWriteTarget::new(target, overwrite),
    )
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
    .map_err(|source| InspectPacksError::Discovery { source })?;

    Ok(InspectPacksResult {
        packs: discovered.iter().map(project_pack).collect(),
    })
}

fn locate_data_home() -> Option<PathBuf> {
    if let Some(project_dirs) = directories::ProjectDirs::from("", "", "KAT") {
        return Some(project_dirs.data_dir().to_path_buf());
    }
    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(app_data).join("KAT").join("data"));
    }
    None
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
    locate_skill_root().map_err(InspectDatasetError::SkillRoot)?;
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
enum InspectPacksError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(
        #[from]
        #[source]
        SkillRootError,
    ),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on a supported platform with a standard user data directory"))]
    DataHomeUnavailable,
    #[error("PACK discovery failed")]
    #[diagnostic(help("Correct the first invalid PACK candidate and retry"))]
    Discovery {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
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
}

#[derive(Debug, Error, Diagnostic)]
enum InspectDatasetError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
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
enum RunOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on a supported platform with a standard user data directory"))]
    DataHomeUnavailable,
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
    OperationLog(OperationLogError),
    #[error("Run Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(OperationLogError),
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

#[derive(Debug, Error, Diagnostic)]
enum QueryOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("KAT Data Home is unavailable on this platform")]
    #[diagnostic(help("Run KAT on a supported platform with a standard user data directory"))]
    DataHomeUnavailable,
    #[error("Query Operation log could not be delivered")]
    #[diagnostic(help("Provide a writable KAT Data Home and retry the complete Query"))]
    OperationLog(#[source] OperationLogError),
    #[error("Query Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(#[source] OperationLogError),
    #[error("Run {run_id} does not exist")]
    #[diagnostic(help("Use the exact Run ID returned by a successful `kat run`"))]
    RunNotFound { run_id: String },
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    CorruptRunPath(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidRunLayout,
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    ReadManifest(#[source] io::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    DecodeManifest(#[source] serde_json::Error),
    #[error("Run is corrupted")]
    #[diagnostic(help("Re-run the Workflow to publish a complete Run"))]
    InvalidManifestFacts,
    #[error("Run path cannot be represented as native Unicode")]
    NonUnicodeRunPath,
    #[error("{label} path cannot be represented as native Unicode: {path:?}")]
    NonUnicodeDatasetPath { label: &'static str, path: PathBuf },
    #[error("failed to encode the candidate Query Response")]
    EncodeCandidate(#[source] serde_json::Error),
    #[error(
        "Query Response byte limit exceeded: candidate is {actual} bytes, limit is {limit} bytes"
    )]
    #[diagnostic(help(
        "Narrow the projection, filter, aggregate, or use an explicit LIMIT, then retry"
    ))]
    ResponseLimit { actual: usize, limit: usize },
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
    fn run_log_diagnostics_hide_the_private_candidate() {
        let candidate_id = "019f6e00-0000-7000-8000-000000000005";
        let error = RunOperationError::IncompleteOperationLog(OperationLogError::Write {
            path: PathBuf::from(format!(r"C:\data\logs\run-{candidate_id}.log")),
            source: io::Error::other("injected log write failure"),
        });

        assert!(std::error::Error::source(&error).is_none());
        assert!(!error.to_string().contains(candidate_id));
    }

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
                workflow_runtime::RuntimeOutput {
                    output_id: "0123456789abcdef0123456789abcdef".to_owned(),
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
