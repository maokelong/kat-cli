use std::{path::Path, sync::Arc};

use crate::{
    operation_log::{OperationLog, OperationLogError},
    response,
    run_manifest::{self, RunManifest},
    session_store::RunAllocation,
    text_projection::project_inline_text,
    workflow_runtime::{
        self, ExchangeError, RunWorkflowInvocation, RunWorkflowOutcome, WorkflowInputs,
    },
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
    pub(super) fn before_runtime(
        log: OperationLog,
        allocation: &RunAllocation,
        error: RunOperationError,
    ) -> Self {
        finish_execution::<()>(log, allocation, Err(ExecutionFailure::Host(error))).unwrap_err()
    }

    pub(super) fn failed_log(
        log: OperationLog,
        allocation: &RunAllocation,
        error: OperationLogError,
    ) -> Self {
        finish_execution::<()>(log, allocation, Err(ExecutionFailure::Log(error))).unwrap_err()
    }

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

enum ExecutionFailure {
    Runtime(Box<response::KatDiagnostic>),
    Host(RunOperationError),
    Log(OperationLogError),
}

// 所有受控退出先收尾，再冻结诊断和日志；Drop 只兜底回收未发布项。
fn finish_execution<T>(
    mut log: OperationLog,
    allocation: &RunAllocation,
    mut result: Result<T, ExecutionFailure>,
) -> Result<(T, OperationLog), RunFailure> {
    if let Err(error) = allocation.finish_scratch() {
        let detail = project_inline_text(&format!("{error:?}"));
        log.append(format!("scratch_cleanup: failure\nerror: {detail}\n").as_bytes())
            .map_err(RunFailure::log_error)?;
        if result.is_ok() {
            result = Err(ExecutionFailure::Host(RunOperationError::SessionStore(
                error,
            )));
        }
    } else {
        log.append(b"scratch_cleanup: success\n")
            .map_err(RunFailure::log_error)?;
    }
    match result {
        Ok(value) => Ok((value, log)),
        Err(ExecutionFailure::Runtime(diagnostic)) => {
            let log_path = log.finish().map_err(RunFailure::log_error)?;
            Err(RunFailure::Runtime {
                diagnostic,
                log_path,
            })
        }
        Err(ExecutionFailure::Host(error)) => Err(RunFailure::logged(log, error)),
        Err(ExecutionFailure::Log(error)) => Err(RunFailure::log_error(error)),
    }
}

/// CLI、ctx.run 和 kat_run 的唯一执行/发布门；调用方保留 allocation 的 lease。
pub(super) fn execute_and_publish(
    mut log: OperationLog,
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
        Err(error) => return Err(RunFailure::before_runtime(log, allocation, error)),
    };
    let result =
        match workflow_runtime::execute_workflow_runtime(&mut log, invocation, coordinator.clone())
        {
            Ok(RunWorkflowOutcome::Success { result }) => Ok(result),
            Ok(RunWorkflowOutcome::Failure { diagnostic }) => {
                Err(ExecutionFailure::Runtime(Box::new(diagnostic)))
            }
            Err(ExchangeError::Log(error)) => Err(ExecutionFailure::Log(error)),
            Err(ExchangeError::Runtime(error)) => {
                Err(ExecutionFailure::Host(RunOperationError::Runtime(error)))
            }
            Err(ExchangeError::InvalidResponse(details)) => {
                match log.append(
                    format!("protocol_failure: {}\n", project_inline_text(&details)).as_bytes(),
                ) {
                    Ok(()) => Err(ExecutionFailure::Host(RunOperationError::Runtime(
                        workflow_runtime::RuntimeInfrastructureError::InvalidResponse,
                    ))),
                    Err(error) => Err(ExecutionFailure::Log(error)),
                }
            }
        };
    let (runtime, mut log) = finish_execution(log, allocation, result)?;
    let manifest = (|| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::{RunId, SessionStore};
    use std::fs;

    #[test]
    fn log_delivery_failure_overrides_business_and_cleanup_failures() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temporary.path());
        let session = store.create().unwrap();
        let run = store
            .create_run_in(session.layout().session_id().as_str(), RunId::generate())
            .unwrap_or_else(|error| panic!("{}", error.error));
        fs::remove_dir(run.scratch()).unwrap();
        fs::write(run.scratch(), b"replacement").unwrap();
        let log =
            OperationLog::create_run(temporary.path(), run.run_id().as_str(), |_| Ok(())).unwrap();
        let log_path = temporary
            .path()
            .join("logs")
            .join(format!("run-{}.log", run.run_id().as_str()));
        fs::remove_file(&log_path).unwrap();
        let diagnostic =
            serde_json::from_value(serde_json::json!({"message": "primary business failure"}))
                .unwrap();
        let failure = finish_execution::<()>(
            log,
            &run,
            Err(ExecutionFailure::Runtime(Box::new(diagnostic))),
        )
        .unwrap_err();
        let RunFailure::Host { report, log_path } = failure else {
            panic!("log failure must own the diagnostic")
        };
        assert!(
            report.to_string().contains("Operation log is incomplete"),
            "{report}"
        );
        assert!(log_path.is_none());
        assert!(fs::symlink_metadata(run.scratch()).is_err());
        assert!(!run.candidate().join("manifest.json").exists());
    }
}
