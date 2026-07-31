use std::{collections::BTreeMap, fs, io, path::PathBuf};

use clap::Args;
use miette::Diagnostic;
use serde::Serialize;
use thiserror::Error;

use crate::{
    locate_data_home,
    operation_log::{OperationLog, OperationLogError},
    pack_discovery, response,
    text_projection::project_inline_text,
    workflow_runtime,
};

#[derive(Args)]
pub(super) struct TestArgs {
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Directory of the exact PACK to test."
    )]
    pack_directory: PathBuf,
    /// Run one exact pytest node ID under tests/. Repeat to select more nodes.
    #[arg(long = "test", value_name = "NODE_ID")]
    tests: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct TestPackResult {
    summary: BTreeMap<String, u64>,
}

pub(super) fn execute(arguments: TestArgs) -> response::PreparedResponse<TestPackResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let token = uuid::Uuid::now_v7().to_string();
    let mut log = match OperationLog::create_test(&data_home, &token, |file| {
        writeln!(
            file,
            "operation: kat test\npack_dir: {:?}\ntests: {:?}",
            arguments.pack_directory, arguments.tests
        )
    }) {
        Ok(log) => log,
        Err(error) => return test_pack_log_failure(error, None),
    };
    let pack = match pack_discovery::discover_pack_at(&arguments.pack_directory) {
        Ok(pack) => pack,
        Err(source) => {
            return finish_test_pack_failure(log, TestPackOperationError::Discovery { source });
        }
    };
    if let Err(error) = log.append(format!("pack: {:?}\n", pack.name()).as_bytes()) {
        return test_pack_log_failure(error, None);
    }
    if !ordinary_directory(&pack.directory().join("tests")) {
        return finish_test_pack_failure(log, TestPackOperationError::MissingTests);
    }
    if let Some(selector) = arguments
        .tests
        .iter()
        .find(|selector| !valid_test_selector(selector))
    {
        return finish_test_pack_failure(
            log,
            TestPackOperationError::InvalidSelector {
                selector: project_inline_text(selector),
            },
        );
    }
    let datasets = match resolve_test_datasets(pack.directory()) {
        Ok(datasets) => datasets,
        Err(error) => return finish_test_pack_failure(log, error),
    };
    let report_directory = data_home.join("test-reports");
    if let Err(source) = fs::create_dir_all(&report_directory) {
        return finish_test_pack_failure(
            log,
            TestPackOperationError::CreateReportDirectory {
                path: report_directory,
                source,
            },
        );
    }
    let report_directory = match dunce::canonicalize(&report_directory) {
        Ok(path) => path,
        Err(source) => {
            return finish_test_pack_failure(
                log,
                TestPackOperationError::ResolveReportDirectory {
                    path: report_directory,
                    source,
                },
            );
        }
    };
    let report_path = report_directory.join(format!("test-{token}.xml"));
    if let Err(error) = log.append(
        format!(
            "path: {:?}\ntest_datasets: {:?}\ntest_report: {:?}\n",
            pack.directory(),
            datasets.keys().collect::<Vec<_>>(),
            report_path
        )
        .as_bytes(),
    ) {
        return test_pack_log_failure(error, None);
    }

    let outcome = workflow_runtime::test_pack(
        log,
        workflow_runtime::TestPackInvocation {
            pack_name: pack.name(),
            pack_path: pack.directory(),
            datasets: &datasets,
            tests: &arguments.tests,
            test_report_path: &report_path,
        },
    );
    match outcome {
        Ok(workflow_runtime::TestPackOutcome::Success { result, log_path }) => {
            let report = match completed_test_report(&report_path) {
                Ok(report) => report,
                Err(error) => {
                    return response::prepare_test_cli_failure(
                        miette::Report::new(error),
                        Some(log_path),
                        None,
                    );
                }
            };
            let Some(report) = report else {
                return response::prepare_test_cli_failure(
                    miette::Report::new(TestPackOperationError::MissingReport),
                    Some(log_path),
                    None,
                );
            };
            response::prepare_test_success(
                TestPackResult {
                    summary: result.summary,
                },
                log_path,
                report,
            )
        }
        Ok(workflow_runtime::TestPackOutcome::Failure {
            diagnostic,
            log_path,
        }) => response::prepare_test_runtime_failure(
            diagnostic,
            log_path,
            completed_test_report(&report_path).ok().flatten(),
        ),
        Err(error) => {
            let log_path = error.log_path();
            response::prepare_test_cli_failure(
                miette::Report::new(error),
                log_path,
                completed_test_report(&report_path).ok().flatten(),
            )
        }
    }
}

fn ordinary_directory(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn valid_test_selector(selector: &str) -> bool {
    let path = selector.split_once("::").map_or(selector, |(path, _)| path);
    if !path.starts_with("tests/") {
        return false;
    }
    !std::path::Path::new(path).components().any(|component| {
        matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::ParentDir
        )
    })
}

fn resolve_test_datasets(
    pack_path: &std::path::Path,
) -> Result<BTreeMap<String, workflow_runtime::ResolvedDatasetRequest>, TestPackOperationError> {
    let root = pack_path.join("tests").join("datasets");
    if !ordinary_directory(&root) {
        return Ok(BTreeMap::new());
    }
    let entries = fs::read_dir(&root).map_err(|source| TestPackOperationError::ReadDatasets {
        path: root.clone(),
        source,
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| TestPackOperationError::ReadDatasets {
            path: root.clone(),
            source,
        })?;
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() || entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            continue;
        }
        match fs::symlink_metadata(path.join(".kat-dataset")) {
            Ok(_) => candidates.push(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => candidates.push(path),
        }
    }
    candidates.sort();
    let mut datasets = BTreeMap::new();
    for candidate in candidates {
        let name = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| TestPackOperationError::NonUnicodeDatasetName {
                path: candidate.clone(),
            })?
            .to_owned();
        let dataset = kat_datasource::resolve_dataset(&candidate).map_err(|source| {
            TestPackOperationError::InvalidDataset {
                name: name.clone(),
                source,
            }
        })?;
        let path = test_unicode_path("Test Dataset", dataset.path())?;
        let mut tables = BTreeMap::new();
        for table in dataset.tables() {
            tables.insert(
                table.name().to_owned(),
                test_unicode_path("Test Dataset table", table.path())?,
            );
        }
        datasets.insert(
            name,
            workflow_runtime::ResolvedDatasetRequest { path, tables },
        );
    }
    Ok(datasets)
}

fn test_unicode_path(
    label: &'static str,
    path: &std::path::Path,
) -> Result<String, TestPackOperationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| TestPackOperationError::NonUnicodeDatasetPath {
            label,
            path: path.to_path_buf(),
        })
}

fn completed_test_report(path: &std::path::Path) -> Result<Option<String>, TestPackOperationError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(TestPackOperationError::InspectReport {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let resolved =
        dunce::canonicalize(path).map_err(|source| TestPackOperationError::ResolveReport {
            path: path.to_path_buf(),
            source,
        })?;
    resolved
        .to_str()
        .map(|path| Some(path.to_owned()))
        .ok_or(TestPackOperationError::NonUnicodeReport { path: resolved })
}

fn finish_test_pack_failure(
    mut log: OperationLog,
    error: TestPackOperationError,
) -> response::PreparedResponse<TestPackResult> {
    if let Err(log_error) = log.append(format!("status: failure\nerror: {error:?}\n").as_bytes()) {
        return test_pack_log_failure(log_error, None);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_test_cli_failure(report, Some(log_path), None),
        Err(error) => test_pack_log_failure(error, None),
    }
}

fn test_pack_log_failure(
    error: OperationLogError,
    test_report_path: Option<String>,
) -> response::PreparedResponse<TestPackResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        TestPackOperationError::IncompleteOperationLog(error)
    } else {
        TestPackOperationError::OperationLog(error)
    };
    response::prepare_test_cli_failure(miette::Report::new(error), log_path, test_report_path)
}

#[derive(Debug, Error, Diagnostic)]
enum TestPackOperationError {
    #[error("PACK test Operation log could not be delivered")]
    #[diagnostic(help("Provide writable storage and retry the complete PACK test"))]
    OperationLog(#[source] OperationLogError),
    #[error("PACK test Operation log is incomplete")]
    #[diagnostic(help(
        "Inspect the partial log if present, then provide writable storage and retry"
    ))]
    IncompleteOperationLog(#[source] OperationLogError),
    #[error("target PACK could not be loaded")]
    #[diagnostic(help("Correct --pack-dir so it names a valid PACK directory and retry"))]
    Discovery {
        #[source]
        source: pack_discovery::PackDiscoveryError,
    },
    #[error("the selected PACK does not contain a tests/ directory")]
    #[diagnostic(help(
        "Add PACK tests, or use `kat inspect --pack` for a runtime-only deployment"
    ))]
    MissingTests,
    #[error("invalid PACK test selector {selector:?}")]
    #[diagnostic(help(
        "Use a pytest node ID whose path begins with tests/ and has no parent-directory component"
    ))]
    InvalidSelector { selector: String },
    #[error("failed to read Test Dataset directory {path:?}")]
    ReadDatasets {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Test Dataset candidate name is not native Unicode: {path:?}")]
    NonUnicodeDatasetName { path: PathBuf },
    #[error("Test Dataset {name:?} is invalid")]
    #[diagnostic(help("Recreate it with `kat import --dataset tests/datasets/<name>`"))]
    InvalidDataset {
        name: String,
        #[source]
        source: kat_datasource::DatasetInspectionError,
    },
    #[error("{label} path is not native Unicode: {path:?}")]
    NonUnicodeDatasetPath { label: &'static str, path: PathBuf },
    #[error("failed to create PACK Test Report directory {path:?}")]
    CreateReportDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve PACK Test Report directory {path:?}")]
    ResolveReportDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect the PACK Test Report {path:?}")]
    InspectReport {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve the PACK Test Report {path:?}")]
    ResolveReport {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("PACK Test Report path is not native Unicode: {path:?}")]
    NonUnicodeReport { path: PathBuf },
    #[error("pytest succeeded without delivering the PACK Test Report")]
    #[diagnostic(help("Inspect the Operation log and repair the bundled pytest deployment"))]
    MissingReport,
}
