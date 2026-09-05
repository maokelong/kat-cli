use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    operation_log::OperationLog,
    pack_discovery,
    pack_discovery::PackDiscoveryPaths,
    run_manifest,
    session_store::{OpenedSession, RunAllocation, RunId, SessionStore},
    text_projection::project_inline_text,
    workflow_runtime::{
        NestedRelation, NestedRunCall, NestedRunCallback, NestedRunOutcome, TestControlCall,
        TestControlCallback, TestControlOutcome, WorkflowInputs,
    },
};

use super::execution::execute_and_publish;

const NESTED_FAILURE: &str = "nested Workflow execution failed";

#[derive(Debug)]
pub(super) struct ChildRunLedgerError;

impl std::fmt::Display for ChildRunLedgerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("nested Workflow child Run ledger is unavailable")
    }
}

impl std::error::Error for ChildRunLedgerError {}

#[derive(Clone, Eq, PartialEq)]
struct WorkflowTarget {
    pack: String,
    workflow: String,
}

#[derive(Clone)]
struct PinnedPack {
    name: String,
    directory: PathBuf,
}

pub(super) struct NestedRunCoordinator {
    data_home: PathBuf,
    log_home: PathBuf,
    session_id: String,
    discovery_paths: PackDiscoveryPaths,
    pinned_pack: Option<PinnedPack>,
    active_chain: Vec<WorkflowTarget>,
    direct_children: Mutex<BTreeSet<String>>,
    log_notes: Mutex<Vec<String>>,
}

impl NestedRunCoordinator {
    pub(super) fn for_root(
        data_home: PathBuf,
        session_id: String,
        discovery_paths: PackDiscoveryPaths,
        pack: String,
        workflow: String,
    ) -> Self {
        Self {
            log_home: data_home.clone(),
            data_home,
            session_id,
            discovery_paths,
            pinned_pack: None,
            active_chain: vec![WorkflowTarget { pack, workflow }],
            direct_children: Mutex::new(BTreeSet::new()),
            log_notes: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn child_runs(&self) -> Result<Vec<String>, ChildRunLedgerError> {
        Ok(self
            .direct_children
            .lock()
            .map_err(|_| ChildRunLedgerError)?
            .iter()
            .cloned()
            .collect())
    }

    fn execute_child(&self, target: WorkflowTarget, input: WorkflowInputs) -> NestedRunOutcome {
        let pack_path = match self.resolve_pack(&target.pack) {
            Ok(path) => path,
            Err(error) => {
                self.record_log(format!("PACK resolution failed: {error}"));
                return nested_failure();
            }
        };
        let run_id = RunId::generate();
        let log = match self.create_log(&run_id, &target) {
            Ok(log) => log,
            Err(_) => return nested_failure(),
        };
        let mut allocation =
            match SessionStore::new(&self.data_home).create_run_in(&self.session_id, run_id) {
                Ok(allocation) => allocation,
                Err(error) => return logged_failure(log, error.error),
            };
        let child_coordinator = Arc::new(self.for_child(target.clone()));
        match execute_and_publish(
            log,
            &mut allocation,
            &target.pack,
            &pack_path,
            &target.workflow,
            input,
            child_coordinator,
        ) {
            Ok(completed) => {
                self.record_log(format!("child Run log: {}", completed.log_path));
                self.deliver_published_child(&allocation)
            }
            Err(error) => {
                self.record_log(format!(
                    "child Workflow failure: {}; log: {}",
                    error.reason(),
                    error.log_path().unwrap_or("unavailable")
                ));
                NestedRunOutcome::Failure {
                    message: error.reason(),
                }
            }
        }
    }

    fn create_log(
        &self,
        run_id: &RunId,
        target: &WorkflowTarget,
    ) -> Result<OperationLog, crate::operation_log::OperationLogError> {
        let pack = project_inline_text(&target.pack);
        let workflow = project_inline_text(&target.workflow);
        OperationLog::create_run(&self.log_home, run_id.as_str(), |file| {
            writeln!(
                file,
                "operation: nested Workflow run\nscope: Runtime-requested child Run\n\
                 publication: manifest.json is the only published Run fact\n\
                 candidate: {}\npack: {}\nworkflow: {}",
                run_id.as_str(),
                pack,
                workflow,
            )
        })
    }

    fn for_child(&self, target: WorkflowTarget) -> Self {
        let mut active_chain = self.active_chain.clone();
        active_chain.push(target);
        Self {
            data_home: self.data_home.clone(),
            log_home: self.log_home.clone(),
            session_id: self.session_id.clone(),
            discovery_paths: self.discovery_paths.clone(),
            pinned_pack: self.pinned_pack.clone(),
            active_chain,
            direct_children: Mutex::new(BTreeSet::new()),
            log_notes: Mutex::new(Vec::new()),
        }
    }

    fn deliver_published_child(&self, allocation: &RunAllocation) -> NestedRunOutcome {
        let run_id = allocation.run_id().as_str().to_owned();
        if self
            .direct_children
            .lock()
            .map_err(|_| ChildRunLedgerError)
            .map(|mut children| children.insert(run_id.clone()))
            .is_err()
        {
            return nested_failure();
        }
        let published = match run_manifest::resolve(allocation.layout(), &run_id) {
            Ok(published) => published,
            Err(_) => return nested_failure(),
        };
        NestedRunOutcome::Success {
            relations: published
                .output_paths
                .into_iter()
                .map(|(name, path)| NestedRelation { name, path })
                .collect(),
        }
    }

    fn record_log(&self, detail: String) {
        self.log_notes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(project_inline_text(&detail));
    }

    fn resolve_pack(&self, name: &str) -> Result<PathBuf, String> {
        if let Some(pinned) = &self.pinned_pack
            && pinned.name == name
        {
            let discovered = pack_discovery::discover_pack_at(&pinned.directory)
                .map_err(|error| error.to_string())?;
            if discovered.name() != name {
                return Err("the exact test PACK identity changed during execution".to_owned());
            }
            return Ok(discovered.directory().to_path_buf());
        }
        let discovered = pack_discovery::discover(self.discovery_paths.clone())
            .map_err(|error| error.to_string())?;
        discovered
            .get(name)
            .map(|pack| pack.directory().to_path_buf())
            .ok_or_else(|| "nested Workflow PACK was not discovered".to_owned())
    }
}

impl NestedRunCallback for NestedRunCoordinator {
    fn take_logs(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .log_notes
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    fn execute(&self, call: NestedRunCall) -> NestedRunOutcome {
        let target = WorkflowTarget {
            pack: call.pack_name,
            workflow: call.workflow_name,
        };
        if self.active_chain.contains(&target) {
            return NestedRunOutcome::Failure {
                message: "recursive Workflow calls are not allowed".to_owned(),
            };
        }
        self.execute_child(target, WorkflowInputs::TypedInputs(call.inputs))
    }
}

struct TestSessionScope {
    _opened: OpenedSession,
    coordinator: NestedRunCoordinator,
}

/// 测试 Session 是临时数据；日志留在正常 Data Home，命令退出后仍可追溯。
pub(crate) struct TestRunCoordinator {
    sessions: Mutex<BTreeMap<String, Arc<TestSessionScope>>>,
    log_notes: Mutex<Vec<String>>,
    data_home: PathBuf,
    log_home: PathBuf,
    discovery_paths: PackDiscoveryPaths,
    pinned_pack: PinnedPack,
    _temporary: tempfile::TempDir,
}

impl TestRunCoordinator {
    pub(crate) fn new(
        discovery_paths: PackDiscoveryPaths,
        exact_pack_name: String,
        exact_pack_directory: &Path,
        log_home: &Path,
    ) -> io::Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix("kat-test-sessions-")
            .tempdir()?;
        let data_home = dunce::canonicalize(temporary.path())?;
        Ok(Self {
            sessions: Mutex::new(BTreeMap::new()),
            log_notes: Mutex::new(Vec::new()),
            data_home,
            log_home: log_home.to_path_buf(),
            discovery_paths,
            pinned_pack: PinnedPack {
                name: exact_pack_name,
                directory: exact_pack_directory.to_path_buf(),
            },
            _temporary: temporary,
        })
    }

    fn begin_session(&self) -> TestControlOutcome {
        let opened = match SessionStore::new(&self.data_home).create() {
            Ok(opened) => opened,
            Err(_) => return test_control_failure(),
        };
        let session_id = opened.layout().session_id().as_str().to_owned();
        let capability = uuid::Uuid::now_v7().to_string();
        let coordinator = NestedRunCoordinator {
            data_home: self.data_home.clone(),
            log_home: self.log_home.clone(),
            session_id,
            discovery_paths: self.discovery_paths.clone(),
            pinned_pack: Some(self.pinned_pack.clone()),
            active_chain: Vec::new(),
            direct_children: Mutex::new(BTreeSet::new()),
            log_notes: Mutex::new(Vec::new()),
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return test_control_failure();
        };
        sessions.insert(
            capability.clone(),
            Arc::new(TestSessionScope {
                _opened: opened,
                coordinator,
            }),
        );
        TestControlOutcome::SessionStarted {
            test_session_id: capability,
        }
    }

    fn execute_workflow(
        &self,
        test_session_id: &str,
        pack: String,
        workflow: String,
        arguments: Vec<String>,
    ) -> TestControlOutcome {
        if pack != self.pinned_pack.name {
            return test_control_failure();
        }
        let session = {
            let Ok(sessions) = self.sessions.lock() else {
                return test_control_failure();
            };
            let Some(scope) = sessions.get(test_session_id) else {
                return test_control_failure();
            };
            Arc::clone(scope)
        };
        let outcome = session.coordinator.execute_child(
            WorkflowTarget { pack, workflow },
            WorkflowInputs::Arguments(arguments),
        );
        self.log_notes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .extend(session.coordinator.take_logs());
        TestControlOutcome::Workflow(outcome)
    }

    fn end_session(&self, test_session_id: &str) -> TestControlOutcome {
        let Ok(mut sessions) = self.sessions.lock() else {
            return test_control_failure();
        };
        if sessions
            .get(test_session_id)
            .is_none_or(|session| Arc::strong_count(session) != 1)
        {
            return test_control_failure();
        }
        sessions.remove(test_session_id);
        TestControlOutcome::Complete
    }
}

impl TestControlCallback for TestRunCoordinator {
    fn take_logs(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .log_notes
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    fn execute(&self, call: TestControlCall) -> TestControlOutcome {
        match call {
            TestControlCall::BeginSession => self.begin_session(),
            TestControlCall::RunWorkflow {
                test_session_id,
                pack_name,
                workflow_name,
                arguments,
            } => self.execute_workflow(&test_session_id, pack_name, workflow_name, arguments),
            TestControlCall::EndSession { test_session_id } => self.end_session(&test_session_id),
        }
    }
}

fn test_control_failure() -> TestControlOutcome {
    TestControlOutcome::Failure {
        message: "PACK test execution scope is unavailable".to_owned(),
    }
}

fn nested_failure() -> NestedRunOutcome {
    NestedRunOutcome::Failure {
        message: NESTED_FAILURE.to_owned(),
    }
}

fn logged_failure(log: OperationLog, error: impl Display) -> NestedRunOutcome {
    let detail = project_inline_text(&error.to_string());
    let mut log = log;
    let _ = log.append(format!("status: failure\nerror: {detail}\n").as_bytes());
    let _ = log.finish();
    nested_failure()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        path::PathBuf,
        sync::Arc,
    };

    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;

    use crate::{
        pack_discovery::PackDiscoveryPaths,
        run_manifest::RunManifest,
        session_store::{RunId, SessionStore},
        workflow_runtime::{
            Column, NestedRunCall, NestedRunCallback, NestedRunOutcome, RunOutputMetadata,
        },
    };

    use super::{NestedRunCoordinator, WorkflowTarget};
    use crate::run::publish_run_manifest;

    #[test]
    fn active_workflow_cannot_be_called_recursively() {
        let coordinator = NestedRunCoordinator::for_root(
            PathBuf::from("unused-data-home"),
            "019f6e00-0000-7000-8000-000000000000".to_owned(),
            PackDiscoveryPaths {
                skill_pack_search_directory: PathBuf::from("unused-skill-packs"),
                data_home_pack_search_directory: PathBuf::from("unused-user-packs"),
                additional_pack_directories: Vec::new(),
            },
            "alpha".to_owned(),
            "analyze".to_owned(),
        );

        let outcome = coordinator.execute(NestedRunCall {
            pack_name: "alpha".to_owned(),
            workflow_name: "analyze".to_owned(),
            inputs: BTreeMap::new(),
        });

        assert!(matches!(
            outcome,
            NestedRunOutcome::Failure { message }
                if message == "recursive Workflow calls are not allowed"
        ));
    }

    #[test]
    fn indirect_active_workflow_cannot_be_called_recursively() {
        let coordinator = NestedRunCoordinator::for_root(
            PathBuf::from("unused-data-home"),
            "019f6e00-0000-7000-8000-000000000000".to_owned(),
            PackDiscoveryPaths {
                skill_pack_search_directory: PathBuf::from("unused-skill-packs"),
                data_home_pack_search_directory: PathBuf::from("unused-user-packs"),
                additional_pack_directories: Vec::new(),
            },
            "alpha".to_owned(),
            "analyze".to_owned(),
        )
        .for_child(WorkflowTarget {
            pack: "beta".to_owned(),
            workflow: "summarize".to_owned(),
        });

        let outcome = coordinator.execute(NestedRunCall {
            pack_name: "alpha".to_owned(),
            workflow_name: "analyze".to_owned(),
            inputs: BTreeMap::new(),
        });

        assert!(matches!(
            outcome,
            NestedRunOutcome::Failure { message }
                if message == "recursive Workflow calls are not allowed"
        ));
    }

    #[test]
    fn a_published_child_stays_in_the_ledger_when_catalog_delivery_fails() {
        let temporary = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temporary.path());
        let opened = store.create().unwrap();
        let session_id = opened.layout().session_id().as_str().to_owned();
        let coordinator = NestedRunCoordinator::for_root(
            temporary.path().to_path_buf(),
            session_id.clone(),
            PackDiscoveryPaths {
                skill_pack_search_directory: temporary.path().join("skill-packs"),
                data_home_pack_search_directory: temporary.path().join("user-packs"),
                additional_pack_directories: Vec::new(),
            },
            "alpha".to_owned(),
            "parent".to_owned(),
        );
        let run_id = RunId::generate();
        let mut allocation = match store.create_run_in(&session_id, run_id.clone()) {
            Ok(allocation) => allocation,
            Err(_) => panic!("create child Run allocation"),
        };
        let outputs = allocation.candidate().join("outputs");
        fs::create_dir(&outputs).unwrap();
        let output = outputs.join("main.parquet");
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int64,
            true,
        )]));
        ArrowWriter::try_new(File::create(&output).unwrap(), schema, None)
            .unwrap()
            .close()
            .unwrap();
        let metadata = BTreeMap::from([(
            "main".to_owned(),
            RunOutputMetadata {
                columns: vec![Column {
                    name: "value".to_owned(),
                    data_type: "int64".to_owned(),
                }],
                row_count: 0,
            },
        )]);
        let manifest = RunManifest::new(
            session_id,
            run_id.as_str().to_owned(),
            "beta".to_owned(),
            "child".to_owned(),
            Vec::new(),
            BTreeMap::new(),
            metadata,
        );
        publish_run_manifest(allocation.candidate(), &manifest).unwrap();
        allocation.mark_run_published();
        fs::remove_file(output).unwrap();

        let outcome = coordinator.deliver_published_child(&allocation);

        assert!(matches!(
            outcome,
            NestedRunOutcome::Failure { message } if message == "nested Workflow execution failed"
        ));
        assert_eq!(
            coordinator.child_runs().unwrap(),
            vec![run_id.as_str().to_owned()]
        );
    }
}
