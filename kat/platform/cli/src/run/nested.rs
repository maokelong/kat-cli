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
    run_manifest::{self, RunManifest},
    session_store::{OpenedSession, RunAllocation, RunId, SessionStore},
    text_projection::project_inline_text,
    workflow_runtime::{
        self, NestedRelation, NestedRunCall, NestedRunCallback, NestedRunOutcome, NestedScalar,
        RunWorkflowInvocation, RunWorkflowOutcome, RunWorkflowReport, TestControlCall,
        TestControlCallback, TestControlOutcome, TestRunCapability,
    },
};

use super::publish_run_manifest;

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
    session_id: String,
    discovery_paths: PackDiscoveryPaths,
    pinned_pack: Option<PinnedPack>,
    active_chain: Vec<WorkflowTarget>,
    direct_children: Mutex<BTreeSet<String>>,
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
            data_home,
            session_id,
            discovery_paths,
            pinned_pack: None,
            active_chain: vec![WorkflowTarget { pack, workflow }],
            direct_children: Mutex::new(BTreeSet::new()),
        }
    }

    fn for_test_root(
        data_home: PathBuf,
        session_id: String,
        discovery_paths: PackDiscoveryPaths,
        pinned_pack: PinnedPack,
        pack: String,
        workflow: String,
    ) -> Self {
        Self {
            data_home,
            session_id,
            discovery_paths,
            pinned_pack: Some(pinned_pack),
            active_chain: vec![WorkflowTarget { pack, workflow }],
            direct_children: Mutex::new(BTreeSet::new()),
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

    fn execute_child(
        &self,
        target: WorkflowTarget,
        inputs: BTreeMap<String, NestedScalar>,
    ) -> NestedRunOutcome {
        let pack_path = match self.resolve_pack(&target.pack) {
            Ok(path) => path,
            Err(_) => return nested_failure(),
        };
        if self.preflight_workflow(&target, &pack_path).is_err() {
            return nested_failure();
        }
        let Some(pack_path) = pack_path.to_str().map(str::to_owned) else {
            return nested_failure();
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
        let Some(candidate_path) = allocation.candidate().to_str().map(str::to_owned) else {
            return logged_failure(log, "nested Run candidate path is not native Unicode");
        };
        let Some(datasource_root) = allocation
            .layout()
            .materializations()
            .to_str()
            .map(str::to_owned)
        else {
            return logged_failure(log, "nested Datasource root is not native Unicode");
        };
        let Some(scratch_root) = allocation.scratch().to_str().map(str::to_owned) else {
            return logged_failure(log, "nested scratch root is not native Unicode");
        };

        let child_coordinator = Arc::new(self.for_child(target.clone()));
        let invocation = RunWorkflowInvocation {
            session_id: self.session_id.clone(),
            pack_name: target.pack.clone(),
            pack_path,
            workflow_name: target.workflow.clone(),
            arguments: Vec::new(),
            candidate_id: allocation.run_id().as_str().to_owned(),
            candidate_path,
            datasource_root,
            scratch_root,
        };
        match workflow_runtime::execute_workflow_runtime_with_inputs(
            log,
            invocation,
            inputs,
            child_coordinator.clone(),
        ) {
            Ok(RunWorkflowOutcome::Success { result, log }) => {
                self.publish_child(&mut allocation, target, result, log, &child_coordinator)
            }
            Ok(RunWorkflowOutcome::Failure { .. }) | Err(_) => nested_failure(),
        }
    }

    fn preflight_workflow(&self, target: &WorkflowTarget, pack_path: &Path) -> Result<(), ()> {
        let pack = project_inline_text(&target.pack);
        let workflow = project_inline_text(&target.workflow);
        let log = OperationLog::create(&self.data_home, "nested-workflow-preflight", |file| {
            writeln!(
                file,
                "operation: nested Workflow preflight\nscope: Runtime-requested child Run\n\
                 pack: {}\nworkflow: {}",
                pack, workflow,
            )
        })
        .map_err(|_| ())?;
        match workflow_runtime::inspect_workflow(
            log,
            &target.pack,
            pack_path,
            Some(&target.workflow),
        ) {
            Ok(workflow_runtime::RuntimeOutcome::Success {
                result: workflow_runtime::WorkflowInspectionResult::Detail(result),
                ..
            }) if result.workflow.name == target.workflow => Ok(()),
            Ok(_) | Err(_) => Err(()),
        }
    }

    fn create_log(
        &self,
        run_id: &RunId,
        target: &WorkflowTarget,
    ) -> Result<OperationLog, crate::operation_log::OperationLogError> {
        let pack = project_inline_text(&target.pack);
        let workflow = project_inline_text(&target.workflow);
        OperationLog::create_run(&self.data_home, run_id.as_str(), |file| {
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
            session_id: self.session_id.clone(),
            discovery_paths: self.discovery_paths.clone(),
            pinned_pack: self.pinned_pack.clone(),
            active_chain,
            direct_children: Mutex::new(BTreeSet::new()),
        }
    }

    fn publish_child(
        &self,
        allocation: &mut RunAllocation,
        target: WorkflowTarget,
        runtime: RunWorkflowReport,
        mut log: OperationLog,
        child_coordinator: &NestedRunCoordinator,
    ) -> NestedRunOutcome {
        if let Err(error) = allocation.clean_scratch() {
            return logged_failure(log, error);
        }
        if let Err(error) =
            run_manifest::validate_candidate_outputs(allocation.candidate(), &runtime.outputs)
        {
            return logged_failure(log, error);
        }
        let child_runs = match child_coordinator.child_runs() {
            Ok(child_runs) => child_runs,
            Err(error) => return logged_failure(log, error),
        };
        let manifest = RunManifest::new(
            self.session_id.clone(),
            allocation.run_id().as_str().to_owned(),
            target.pack,
            target.workflow,
            child_runs,
            runtime.effective_inputs,
            runtime.outputs,
        );
        if log.append(b"publication_gate: ready\n").is_err() {
            let _ = log.finish();
            return nested_failure();
        }
        if log.finish().is_err() {
            return nested_failure();
        }
        if publish_run_manifest(allocation.candidate(), &manifest).is_err() {
            return nested_failure();
        }
        allocation.mark_run_published();

        self.deliver_published_child(allocation)
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
        self.execute_child(target, call.inputs)
    }
}

struct TestSessionScope {
    session_id: String,
    _opened: OpenedSession,
}

struct TestWorkflowScope {
    test_session_id: String,
    _allocation: RunAllocation,
    coordinator: Arc<NestedRunCoordinator>,
}

#[derive(Default)]
struct TestCoordinatorState {
    sessions: BTreeMap<String, TestSessionScope>,
    workflows: BTreeMap<String, TestWorkflowScope>,
}

/// Owns all ephemeral Session storage for one `kat test` Host process.
///
/// The Python Runtime receives only opaque scope IDs and individual path
/// capabilities allocated here. The normal configured Data Home is used only
/// as one PACK discovery root and never receives test Sessions or child Runs.
pub(crate) struct TestRunCoordinator {
    state: Mutex<TestCoordinatorState>,
    data_home: PathBuf,
    discovery_paths: PackDiscoveryPaths,
    pinned_pack: PinnedPack,
    _temporary: tempfile::TempDir,
}

impl TestRunCoordinator {
    pub(crate) fn new(
        discovery_paths: PackDiscoveryPaths,
        exact_pack_name: String,
        exact_pack_directory: &Path,
    ) -> io::Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix("kat-test-sessions-")
            .tempdir()?;
        let data_home = dunce::canonicalize(temporary.path())?;
        Ok(Self {
            state: Mutex::new(TestCoordinatorState::default()),
            data_home,
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
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return test_control_failure(),
        };
        if state
            .sessions
            .insert(
                capability.clone(),
                TestSessionScope {
                    session_id,
                    _opened: opened,
                },
            )
            .is_some()
        {
            return test_control_failure();
        }
        TestControlOutcome::SessionStarted {
            test_session_id: capability,
        }
    }

    fn begin_workflow(
        &self,
        test_session_id: String,
        pack_name: String,
        workflow_name: String,
    ) -> TestControlOutcome {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return test_control_failure(),
        };
        let Some(session) = state.sessions.get(&test_session_id) else {
            return test_control_failure();
        };
        let session_id = session.session_id.clone();
        let run_id = RunId::generate();
        let allocation = match SessionStore::new(&self.data_home).create_run_in(&session_id, run_id)
        {
            Ok(allocation) => allocation,
            Err(_) => return test_control_failure(),
        };
        let capability = uuid::Uuid::now_v7().to_string();
        let Some(candidate_path) = allocation.candidate().to_str().map(str::to_owned) else {
            return test_control_failure();
        };
        let Some(datasource_root) = allocation
            .layout()
            .materializations()
            .to_str()
            .map(str::to_owned)
        else {
            return test_control_failure();
        };
        let Some(scratch_root) = allocation.scratch().to_str().map(str::to_owned) else {
            return test_control_failure();
        };
        let candidate_id = allocation.run_id().as_str().to_owned();
        let coordinator = Arc::new(NestedRunCoordinator::for_test_root(
            self.data_home.clone(),
            session_id,
            self.discovery_paths.clone(),
            self.pinned_pack.clone(),
            pack_name,
            workflow_name,
        ));
        if state
            .workflows
            .insert(
                capability.clone(),
                TestWorkflowScope {
                    test_session_id,
                    _allocation: allocation,
                    coordinator,
                },
            )
            .is_some()
        {
            return test_control_failure();
        }
        TestControlOutcome::RunStarted(TestRunCapability {
            test_run_id: capability,
            candidate_id,
            candidate_path,
            datasource_root,
            scratch_root,
        })
    }

    fn execute_workflow(&self, test_run_id: &str, call: NestedRunCall) -> TestControlOutcome {
        let coordinator = {
            let state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return test_control_failure(),
            };
            match state.workflows.get(test_run_id) {
                Some(scope) => Arc::clone(&scope.coordinator),
                None => return test_control_failure(),
            }
        };
        TestControlOutcome::Workflow(coordinator.execute(call))
    }

    fn end_workflow(&self, test_run_id: &str) -> TestControlOutcome {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return test_control_failure(),
        };
        if state.workflows.remove(test_run_id).is_none() {
            return test_control_failure();
        }
        TestControlOutcome::Complete
    }

    fn end_session(&self, test_session_id: &str) -> TestControlOutcome {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return test_control_failure(),
        };
        if state
            .workflows
            .values()
            .any(|workflow| workflow.test_session_id == test_session_id)
            || state.sessions.remove(test_session_id).is_none()
        {
            return test_control_failure();
        }
        TestControlOutcome::Complete
    }
}

impl TestControlCallback for TestRunCoordinator {
    fn execute(&self, call: TestControlCall) -> TestControlOutcome {
        match call {
            TestControlCall::BeginSession => self.begin_session(),
            TestControlCall::BeginRun {
                test_session_id,
                pack_name,
                workflow_name,
            } => self.begin_workflow(test_session_id, pack_name, workflow_name),
            TestControlCall::RunWorkflow { test_run_id, call } => {
                self.execute_workflow(&test_run_id, call)
            }
            TestControlCall::EndRun { test_run_id } => self.end_workflow(&test_run_id),
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

    use super::{NestedRunCoordinator, WorkflowTarget, publish_run_manifest};

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
        fs::write(output, b"externally corrupted after publication").unwrap();

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
