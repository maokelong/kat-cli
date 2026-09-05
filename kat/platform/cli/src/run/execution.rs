use std::{path::Path, sync::Arc};

use crate::{
    operation_log::OperationLog,
    response,
    run_manifest::{self, RunManifest},
    session_store::RunAllocation,
    text_projection::project_inline_text,
    workflow_runtime::{self, RunWorkflowInvocation, RunWorkflowOutcome, WorkflowInputs},
};

use super::{RunOperationError, nested::NestedRunCoordinator, publish_run_manifest};

pub(super) struct CompletedRun {
    pub(super) manifest: RunManifest,
    pub(super) log_path: String,
}

pub(super) enum RunFailure {
    Runtime {
        diagnostic: Box<response::KatDiagnostic>,
        log_path: String,
    },
    Host {
        report: miette::Report,
        log_path: Option<String>,
    },
}

impl RunFailure {
    pub(super) fn into_response<P>(self) -> response::PreparedResponse<P> {
        match self {
            Self::Runtime {
                diagnostic,
                log_path,
            } => response::prepare_runtime_failure(*diagnostic, log_path),
            Self::Host { report, log_path } => {
                response::prepare_cli_failure_with_log(report, log_path)
            }
        }
    }

    pub(super) fn reason(&self) -> String {
        match self {
            Self::Runtime { diagnostic, .. } => diagnostic.reason(),
            Self::Host { .. } => "nested Workflow execution failed".to_owned(),
        }
    }

    pub(super) fn log_path(&self) -> Option<&str> {
        match self {
            Self::Runtime { log_path, .. } => Some(log_path),
            Self::Host { log_path, .. } => log_path.as_deref(),
        }
    }

    fn logged(mut log: OperationLog, error: RunOperationError) -> Self {
        let detail = project_inline_text(&format!("{error:?}"));
        if let Err(error) = log.append(format!("status: failure\nerror: {detail}\n").as_bytes()) {
            return Self::log_error(error);
        }
        match log.finish() {
            Ok(log_path) => Self::Host {
                report: miette::Report::new(error),
                log_path: Some(log_path),
            },
            Err(error) => Self::log_error(error),
        }
    }

    fn log_error(error: crate::operation_log::OperationLogError) -> Self {
        let log_path = error.readable_path();
        Self::Host {
            report: miette::Report::new(RunOperationError::IncompleteOperationLog(error)),
            log_path,
        }
    }
}

/// CLI、ctx.run 和 kat_run 的唯一执行/发布门；调用方保留 allocation 的 lease。
pub(super) fn execute_and_publish(
    log: OperationLog,
    allocation: &mut RunAllocation,
    pack: &str,
    pack_path: &Path,
    workflow: &str,
    input: WorkflowInputs,
    coordinator: Arc<NestedRunCoordinator>,
) -> Result<CompletedRun, RunFailure> {
    let invocation = (|| {
        Ok(RunWorkflowInvocation {
            session_id: allocation.layout().session_id().as_str().to_owned(),
            pack_name: pack.to_owned(),
            pack_path: pack_path
                .to_str()
                .ok_or_else(|| RunOperationError::NonUnicodePath {
                    label: "PACK",
                    path: pack_path.to_path_buf(),
                })?
                .to_owned(),
            workflow_name: workflow.to_owned(),
            input,
            candidate_id: allocation.run_id().as_str().to_owned(),
            candidate_path: allocation
                .candidate()
                .to_str()
                .ok_or(RunOperationError::PrivateCandidatePath)?
                .to_owned(),
            datasource_root: allocation
                .layout()
                .materializations()
                .to_str()
                .ok_or(RunOperationError::PrivateDatasourceRootPath)?
                .to_owned(),
            scratch_root: allocation
                .scratch()
                .to_str()
                .ok_or(RunOperationError::PrivateScratchRootPath)?
                .to_owned(),
        })
    })();
    let invocation = match invocation {
        Ok(invocation) => invocation,
        Err(error) => return Err(RunFailure::logged(log, error)),
    };
    let (runtime, mut log) =
        match workflow_runtime::execute_workflow_runtime(log, invocation, coordinator.clone()) {
            Ok(RunWorkflowOutcome::Success { result, log }) => (result, log),
            Ok(RunWorkflowOutcome::Failure {
                diagnostic,
                log_path,
            }) => {
                return Err(RunFailure::Runtime {
                    diagnostic: Box::new(diagnostic),
                    log_path,
                });
            }
            Err(error) => {
                let log_path = error.log_path();
                return Err(RunFailure::Host {
                    report: miette::Report::new(error),
                    log_path,
                });
            }
        };
    let manifest = (|| {
        allocation
            .clean_scratch()
            .map_err(RunOperationError::SessionStore)?;
        run_manifest::validate_candidate_outputs(allocation.candidate(), &runtime.outputs)
            .map_err(|source| RunOperationError::InvalidOutputLayout { source })?;
        let child_runs = coordinator
            .child_runs()
            .map_err(|source| RunOperationError::ChildRunLedger { source })?;
        Ok(RunManifest::new(
            allocation.layout().session_id().as_str().to_owned(),
            allocation.run_id().as_str().to_owned(),
            pack.to_owned(),
            workflow.to_owned(),
            child_runs,
            runtime.effective_inputs,
            runtime.outputs,
        ))
    })();
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => return Err(RunFailure::logged(log, error)),
    };
    log.append(b"publication_gate: ready\n")
        .map_err(RunFailure::log_error)?;
    let log_path = log.finish().map_err(RunFailure::log_error)?;
    publish_run_manifest(allocation.candidate(), &manifest).map_err(|error| RunFailure::Host {
        report: miette::Report::new(error),
        log_path: Some(log_path.clone()),
    })?;
    allocation.mark_run_published();
    Ok(CompletedRun { manifest, log_path })
}
