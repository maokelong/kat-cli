use std::{
    fs, io,
    path::{Path, PathBuf},
};

use clap::Args;
use miette::Diagnostic;
use serde::Serialize;
use thiserror::Error;

use crate::{
    SkillRootError, locate_data_home, locate_skill_root,
    operation_log::{OperationLog, OperationLogError},
    response,
    run::RunManifest,
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
    /// Local read sources and resource use are the user's responsibility. The complete
    /// SQL value is retained in the Query Operation log.
    #[arg(long, value_name = "SQL")]
    sql: String,
}

#[derive(Serialize)]
pub(super) struct QueryResult {
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

enum QueryDatasetState {
    NotProvided,
    Available(workflow_runtime::ResolvedDatasetRequest),
    Unavailable {
        path: String,
        cause: String,
    },
}

impl QueryDatasetState {
    fn into_runtime_and_result(
        self,
    ) -> (
        Option<workflow_runtime::ResolvedDatasetRequest>,
        QueryDatasetResult,
    ) {
        match self {
            Self::NotProvided => (None, QueryDatasetResult::NotProvided),
            Self::Available(dataset) => {
                let public_path = dataset.path.clone();
                (
                    Some(dataset),
                    QueryDatasetResult::Available { path: public_path },
                )
            }
            Self::Unavailable { path, cause } => {
                (
                    None,
                    QueryDatasetResult::Unavailable {
                        path,
                        cause,
                    },
                )
            }
        }
    }
}

pub(super) fn execute(arguments: QueryArgs) -> response::PreparedResponse<QueryResult> {
    let Some(data_home) = locate_data_home() else {
        return response::prepare_cli_failure(miette::Report::new(
            QueryOperationError::DataHomeUnavailable,
        ));
    };
    let run_log = project_inline_text(&format!("{:?}", arguments.run));
    let sql_log = project_inline_text(&format!("{:?}", arguments.sql));
    let mut log = match OperationLog::create(&data_home, "query", |file| {
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
    let dataset = resolve_dataset(manifest.dataset.as_deref());
    let Some(run_path_text) = run_path.to_str().map(str::to_owned) else {
        return finish_failure(log, QueryOperationError::NonUnicodeRunPath);
    };
    let outputs = manifest.outputs.keys().cloned().collect::<Vec<_>>();
    let outputs_log = match serde_json::to_string(&outputs) {
        Ok(value) => value,
        Err(source) => {
            return finish_failure(log, QueryOperationError::EncodeLogEvidence(source));
        }
    };
    let run_path_log = project_inline_text(&format!("{run_path:?}"));
    let dataset_log = dataset_log(&dataset);
    if let Err(error) = log.append(
        format!("run_path: {run_path_log}\n{dataset_log}outputs: {outputs_log}\n").as_bytes(),
    ) {
        return log_failure(error);
    }
    let (runtime_dataset, public_dataset) = dataset.into_runtime_and_result();
    let outcome = workflow_runtime::execute_query_runtime(
        log,
        workflow_runtime::QueryRunInvocation {
            run_path: run_path_text,
            outputs,
            dataset: runtime_dataset,
            sql: arguments.sql,
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
    let result = QueryResult {
        dataset: public_dataset,
        columns: runtime.columns,
        rows: runtime.rows,
    };
    if let Err(error) = log.append(b"status: success\n") {
        return log_failure(error);
    }
    match log.finish() {
        Ok(log_path) => response::prepare_success_with_log(result, Some(log_path)),
        Err(error) => log_failure(error),
    }
}

fn read_run_manifest(
    data_home: &Path,
    run_id: &str,
) -> Result<(PathBuf, RunManifest), QueryOperationError> {
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
    let manifest: RunManifest =
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

fn validate_run_manifest(manifest: &RunManifest, run_id: &str) -> Result<(), QueryOperationError> {
    if manifest.run_id != run_id
        || manifest.pack.trim().is_empty()
        || manifest.workflow.trim().is_empty()
        || manifest.outputs.is_empty()
        || manifest
            .dataset
            .as_ref()
            .is_some_and(|path| path.is_empty() || !Path::new(path).is_absolute())
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

fn resolve_dataset(recorded_path: Option<&str>) -> QueryDatasetState {
    let Some(recorded_path) = recorded_path else {
        return QueryDatasetState::NotProvided;
    };
    classify_dataset_resolution(recorded_path, || {
        let dataset = kat_datasource::resolve_dataset(Path::new(recorded_path))
            .map_err(|source| error_chain(&source))?;
        workflow_runtime::project_dataset(&dataset).map_err(|source| error_chain(&source))
    })
}

fn classify_dataset_resolution(
    recorded_path: &str,
    resolve: impl FnOnce() -> Result<workflow_runtime::ResolvedDatasetRequest, String>,
) -> QueryDatasetState {
    match resolve() {
        Ok(resolved) => QueryDatasetState::Available(resolved),
        Err(cause) => QueryDatasetState::Unavailable {
            path: recorded_path.to_owned(),
            cause,
        },
    }
}

fn dataset_log(dataset: &QueryDatasetState) -> String {
    match dataset {
        QueryDatasetState::NotProvided => "dataset_status: not_provided\n".to_owned(),
        QueryDatasetState::Available(dataset) => format!(
            "dataset_status: available\ndataset_path: {}\n",
            project_inline_text(&format!("{:?}", dataset.path))
        ),
        QueryDatasetState::Unavailable { path, cause } => format!(
            "dataset_status: unavailable\ndataset_path: {}\ndataset_cause: {}\n",
            project_inline_text(&format!("{path:?}")),
            project_inline_text(&format!("{cause:?}"))
        ),
    }
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !cause_text.trim().is_empty() {
            rendered.push_str(": ");
            rendered.push_str(&cause_text);
        }
        source = cause.source();
    }
    rendered
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
    #[error("failed to encode Query Operation log evidence")]
    EncodeLogEvidence(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_projection_failure_becomes_unavailable_current_state() {
        let recorded_path = "/recorded/dataset";
        let state = classify_dataset_resolution(recorded_path, || {
            Err("Dataset table path cannot be represented as native Unicode".to_owned())
        });

        assert_eq!(
            dataset_log(&state),
            concat!(
                "dataset_status: unavailable\n",
                "dataset_path: \"/recorded/dataset\"\n",
                "dataset_cause: \"Dataset table path cannot be represented as native Unicode\"\n",
            )
        );
        let (runtime, public) = state.into_runtime_and_result();
        assert!(runtime.is_none());
        assert!(matches!(
            public,
            QueryDatasetResult::Unavailable {
                ref path,
                ref cause,
            } if path == recorded_path
                && cause == "Dataset table path cannot be represented as native Unicode"
        ));
    }
}
