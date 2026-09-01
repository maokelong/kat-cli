use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use clap::Args;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    SkillRootError, locate_data_home, locate_skill_root,
    operation_log::{OperationLog, OperationLogError},
    response::{self, PendingResponseFile},
    text_projection::project_inline_text,
    workflow_runtime,
};

#[derive(Args)]
pub(super) struct QueryArgs {
    /// Select one exact published Run ID.
    #[arg(long, value_name = "RUN_ID")]
    run: String,
    /// Execute one unmodified DataFusion SQL query without changing KAT-managed state.
    ///
    /// Only this Run's registered output.* relations are available. The complete SQL
    /// value is retained in the Query Operation log.
    #[arg(long, value_name = "SQL")]
    sql: String,
}

#[derive(Serialize)]
pub(super) struct QueryResult {
    format: &'static str,
    path: String,
    columns: Vec<workflow_runtime::Column>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRunManifest {
    run_id: String,
    pack: String,
    workflow: String,
    #[serde(
        default,
        rename = "dataset",
        deserialize_with = "deserialize_ignored_manifest_dataset"
    )]
    _dataset: (),
    inputs: BTreeMap<String, serde_json::Value>,
    outputs: BTreeMap<String, workflow_runtime::RunOutputMetadata>,
}

fn deserialize_ignored_manifest_dataset<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::IgnoredAny::deserialize(deserializer).map(drop)
}

pub(super) fn execute(arguments: QueryArgs) -> response::PreparedResponse<QueryResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let operation_id = uuid::Uuid::now_v7().to_string();
    let run_log = project_inline_text(&format!("{:?}", arguments.run));
    let sql_log = project_inline_text(&format!("{:?}", arguments.sql));
    let mut log = match OperationLog::create_query(&data_home, &operation_id, |file| {
        writeln!(file, "operation: kat query\nrun: {run_log}\nsql: {sql_log}")
    }) {
        Ok(log) => log,
        Err(error) => return log_failure(error),
    };
    if let Err(source) = locate_skill_root() {
        return finish_failure(log, QueryOperationError::SkillRoot(source));
    }
    let (run_path, manifest) = match read_run_manifest(&data_home, &arguments.run) {
        Ok(value) => value,
        Err(error) => return finish_failure(log, error),
    };
    let outputs = match resolve_outputs(&run_path, &manifest) {
        Ok(outputs) => outputs,
        Err(error) => return finish_failure(log, error),
    };
    let output_names = outputs.keys().cloned().collect::<Vec<_>>();
    let outputs_log = match serde_json::to_string(&output_names) {
        Ok(value) => value,
        Err(source) => {
            return finish_failure(log, QueryOperationError::EncodeLogEvidence(source));
        }
    };
    let run_path_log = project_inline_text(&format!("{run_path:?}"));
    if let Err(error) =
        log.append(format!("run_path: {run_path_log}\noutputs: {outputs_log}\n").as_bytes())
    {
        return log_failure(error);
    }
    let result_path = match allocate_result_candidate(&data_home, &operation_id) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let pending_file = PendingResponseFile::new(result_path.clone());
    let Some(result_path_text) = result_path.to_str().map(str::to_owned) else {
        return finish_failure(log, QueryOperationError::NonUnicodeResultPath);
    };
    let outcome = workflow_runtime::execute_query_runtime(
        log,
        workflow_runtime::QueryRunInvocation {
            outputs,
            sql: arguments.sql,
            result_path: result_path_text.clone(),
        },
    );
    let (runtime, mut log) = match outcome {
        Ok(workflow_runtime::QueryRunOutcome::Success { result, log }) => (result, log),
        Ok(workflow_runtime::QueryRunOutcome::Failure {
            diagnostic,
            mut log,
        }) => {
            let runtime_diagnostic = match serde_json::to_string(&diagnostic) {
                Ok(value) => value,
                Err(source) => {
                    return finish_failure(log, QueryOperationError::EncodeLogEvidence(source));
                }
            };
            if let Err(error) = log.append(
                format!("runtime_diagnostic: {runtime_diagnostic}\nstatus: failure\n").as_bytes(),
            ) {
                return log_failure(error);
            }
            return match log.finish() {
                Ok(log_path) => response::prepare_runtime_failure(diagnostic, log_path),
                Err(error) => log_failure(error),
            };
        }
        Err(error) => {
            let log_path = error.log_path();
            return response::prepare_cli_failure_with_log(miette::Report::new(error), log_path);
        }
    };
    if let Err(error) = validate_result_file(&result_path) {
        return finish_failure(log, error);
    }
    let result = QueryResult {
        format: "ndjson",
        path: result_path_text,
        columns: runtime.columns,
    };
    if let Err(error) = log.append(b"status: success\n") {
        return log_failure(error);
    }
    match log.finish() {
        Ok(log_path) => {
            response::prepare_success_with_log_and_file(result, Some(log_path), pending_file)
        }
        Err(error) => log_failure(error),
    }
}

fn read_run_manifest(
    data_home: &Path,
    run_id: &str,
) -> Result<(PathBuf, QueryRunManifest), QueryOperationError> {
    uuid::Uuid::parse_str(run_id)
        .ok()
        .filter(|identity| identity.get_version_num() == 7 && identity.to_string() == run_id)
        .ok_or_else(|| QueryOperationError::RunNotFound {
            run_id: diagnostic_safe_argument(run_id),
        })?;
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
    let manifest: QueryRunManifest =
        serde_json::from_slice(&bytes).map_err(QueryOperationError::DecodeManifest)?;
    validate_run_manifest(&manifest, run_id)?;
    Ok((run_path, manifest))
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

fn validate_run_manifest(
    manifest: &QueryRunManifest,
    run_id: &str,
) -> Result<(), QueryOperationError> {
    if manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
    {
        return Err(QueryOperationError::InvalidManifestFacts);
    }
    for (name, output) in &manifest.outputs {
        if !workflow_runtime::valid_output_name(name)
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

fn resolve_outputs(
    run_path: &Path,
    manifest: &QueryRunManifest,
) -> Result<BTreeMap<String, String>, QueryOperationError> {
    let output_directory = run_path.join("outputs");
    let metadata =
        fs::symlink_metadata(&output_directory).map_err(QueryOperationError::CorruptRunPath)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(QueryOperationError::InvalidRunLayout);
    }
    let output_directory =
        dunce::canonicalize(&output_directory).map_err(QueryOperationError::CorruptRunPath)?;
    if output_directory.parent() != Some(run_path)
        || output_directory.file_name().and_then(|name| name.to_str()) != Some("outputs")
    {
        return Err(QueryOperationError::InvalidRunLayout);
    }

    let mut outputs = BTreeMap::new();
    for name in manifest.outputs.keys() {
        let candidate = output_directory.join(format!("{name}.parquet"));
        let metadata =
            fs::symlink_metadata(&candidate).map_err(QueryOperationError::CorruptRunPath)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(QueryOperationError::InvalidRunLayout);
        }
        let resolved =
            dunce::canonicalize(&candidate).map_err(QueryOperationError::CorruptRunPath)?;
        if resolved.parent() != Some(output_directory.as_path())
            || resolved.file_name() != candidate.file_name()
        {
            return Err(QueryOperationError::InvalidRunLayout);
        }
        let Some(path) = resolved.to_str().map(str::to_owned) else {
            return Err(QueryOperationError::NonUnicodeRunPath);
        };
        outputs.insert(name.clone(), path);
    }
    Ok(outputs)
}

fn allocate_result_candidate(
    data_home: &Path,
    operation_id: &str,
) -> Result<PathBuf, QueryOperationError> {
    let directory = data_home.join("query-results");
    fs::create_dir_all(&directory).map_err(QueryOperationError::AllocateResult)?;
    let metadata = fs::symlink_metadata(&directory).map_err(QueryOperationError::AllocateResult)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(QueryOperationError::InvalidResultStorage);
    }
    let directory = dunce::canonicalize(&directory).map_err(QueryOperationError::AllocateResult)?;
    let data_home = dunce::canonicalize(data_home).map_err(QueryOperationError::AllocateResult)?;
    if directory.parent() != Some(data_home.as_path())
        || directory.file_name().and_then(|name| name.to_str()) != Some("query-results")
    {
        return Err(QueryOperationError::InvalidResultStorage);
    }
    let candidate = directory.join(format!("query-{operation_id}.ndjson"));
    match fs::symlink_metadata(&candidate) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(candidate),
        Err(error) => Err(QueryOperationError::AllocateResult(error)),
        Ok(_) => Err(QueryOperationError::ResultAlreadyExists),
    }
}

fn validate_result_file(path: &Path) -> Result<(), QueryOperationError> {
    let metadata = fs::symlink_metadata(path).map_err(QueryOperationError::MissingResult)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(QueryOperationError::InvalidResultFile);
    }
    let resolved = dunce::canonicalize(path).map_err(QueryOperationError::MissingResult)?;
    if resolved != path {
        return Err(QueryOperationError::InvalidResultFile);
    }
    Ok(())
}

fn finish_failure(
    mut log: OperationLog,
    error: QueryOperationError,
) -> response::PreparedResponse<QueryResult> {
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

fn log_failure(error: OperationLogError) -> response::PreparedResponse<QueryResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        QueryOperationError::IncompleteOperationLog(error)
    } else {
        QueryOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

#[derive(Debug, Error, Diagnostic)]
enum QueryOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
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
    #[error("Query Result storage is unavailable")]
    #[diagnostic(help("Provide writable local storage and retry the complete Query"))]
    AllocateResult(#[source] io::Error),
    #[error("Query Result storage layout is invalid")]
    #[diagnostic(help("Replace linked or conflicting Query Result storage and retry"))]
    InvalidResultStorage,
    #[error("Query Result candidate already exists")]
    #[diagnostic(help("Remove the conflicting candidate and retry the complete Query"))]
    ResultAlreadyExists,
    #[error("Query Result path cannot be represented as native Unicode")]
    NonUnicodeResultPath,
    #[error("Runtime did not publish the Query Result")]
    #[diagnostic(help("Inspect the Query Operation log, then retry the complete Query"))]
    MissingResult(#[source] io::Error),
    #[error("Runtime published an invalid Query Result")]
    #[diagnostic(help("Inspect the Query Operation log, then retry the complete Query"))]
    InvalidResultFile,
    #[error("failed to encode Query Operation log evidence")]
    EncodeLogEvidence(#[source] serde_json::Error),
}
