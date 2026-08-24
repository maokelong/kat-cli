use std::{
    io,
    path::{Path, PathBuf},
};

use clap::Args;
use miette::Diagnostic;
use serde::Serialize;
use thiserror::Error;

use crate::{
    SkillRootError, locate_data_home, locate_skill_root,
    operation_log::{OperationLog, OperationLogError},
    pack_discovery::{self, PackDiscoveryPaths},
    response,
    text_projection::project_inline_text,
    workflow_runtime,
};

#[derive(Args)]
pub(super) struct MaterializeArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Source name from the PACK.
    #[arg(long, value_name = "NAME")]
    source: String,
    /// Create or update this KAT Dataset directory.
    #[arg(long, value_name = "DIRECTORY")]
    dataset: PathBuf,
    /// Materialize one table. Repeat to select multiple tables; omit to materialize all tables.
    #[arg(long = "table", value_name = "NAME")]
    tables: Vec<String>,
    /// Replace an existing Binding for the same PACK and Source.
    #[arg(long)]
    replace: bool,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add a PACK candidate directory for this command. Repeat to add more candidate directories."
    )]
    pack_directories: Vec<PathBuf>,
    /// Forward all tokens after `--` unchanged to the Source Input Compiler.
    /// The Operation log may retain the complete vector.
    #[arg(last = true, value_name = "ARGUMENT")]
    source_arguments: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct MaterializeResult {
    path: String,
    pack: String,
    source: String,
    kind: &'static str,
    tables: Vec<String>,
}

pub(super) fn execute(arguments: MaterializeArgs) -> response::PreparedResponse<MaterializeResult> {
    let data_home = match locate_data_home() {
        Ok(path) => path,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let log = match OperationLog::create(&data_home, "materialize", |file| {
        writeln!(
            file,
            "operation: kat materialize\npack: {}\nsource: {}\ndataset: {:?}\ntables: {:?}\narguments: {:?}",
            project_inline_text(&arguments.pack),
            project_inline_text(&arguments.source),
            arguments.dataset,
            arguments.tables,
            arguments.source_arguments,
        )
    }) {
        Ok(log) => log,
        Err(error) => return log_failure(error),
    };

    let tables = match normalized_tables(arguments.tables) {
        Ok(tables) => tables,
        Err(error) => return finish_failure(log, error),
    };
    let target = match kat_datasource::inspect_dataset_target(
        &arguments.dataset,
        &arguments.pack,
        &arguments.source,
    ) {
        Ok(target) => target,
        Err(source) => return finish_failure(log, MaterializeOperationError::Dataset { source }),
    };
    if let Some(kind) = target.binding()
        && !arguments.replace
    {
        return finish_failure(
            log,
            MaterializeOperationError::BindingExists {
                pack: arguments.pack,
                source_name: arguments.source,
                kind: kind.as_str(),
            },
        );
    }

    let current_directory = match canonical_current_directory() {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let (source_arguments, argument_base) = match select_source_arguments(
        &arguments.source_arguments,
        &current_directory,
        target.resolved_binding(),
    ) {
        Ok(selected) => selected,
        Err(error) => return finish_failure(log, error),
    };
    let recipe_working_directory = argument_base.clone();
    let argument_base = match unicode_path("Source argument base", argument_base) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };

    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => {
            return finish_failure(log, MaterializeOperationError::SkillRoot(source));
        }
    };
    let discovered = match pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: arguments.pack_directories,
    }) {
        Ok(packs) => packs,
        Err(source) => {
            return finish_failure(log, MaterializeOperationError::Discovery { source });
        }
    };
    let Some(pack) = discovered.get(&arguments.pack) else {
        return finish_failure(
            log,
            MaterializeOperationError::UnknownPack {
                name: arguments.pack,
            },
        );
    };
    let pack_path = match unicode_path("PACK", pack.directory().to_path_buf()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };

    let export = match tempfile::Builder::new()
        .prefix("kat-materialize-export-")
        .tempdir()
    {
        Ok(directory) => directory,
        Err(source) => {
            return finish_failure(
                log,
                MaterializeOperationError::CreateExportDirectory { source },
            );
        }
    };
    let export_path = match dunce::canonicalize(export.path()) {
        Ok(path) => path,
        Err(source) => {
            return finish_failure(
                log,
                MaterializeOperationError::CanonicalizeExportDirectory { source },
            );
        }
    };
    let export_path_text = match unicode_path("private materialize export", export_path.clone()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let invocation = workflow_runtime::MaterializeSourceInvocation {
        pack_name: pack.name().to_owned(),
        pack_path,
        source_name: arguments.source.clone(),
        arguments: source_arguments.clone(),
        argument_base,
        tables: tables.clone(),
        export_path: export_path_text,
    };
    let outcome = match workflow_runtime::materialize_source(log, invocation) {
        Ok(outcome) => outcome,
        Err(error) => {
            let log_path = error.log_path();
            return response::prepare_cli_failure_with_log(miette::Report::new(error), log_path);
        }
    };
    let (runtime, mut log) = match outcome {
        workflow_runtime::SourceRuntimeOutcome::Success { result, log } => (result, log),
        workflow_runtime::SourceRuntimeOutcome::Failure {
            diagnostic,
            mut log,
        } => {
            if let Err(error) = log.append(b"status: failure\n") {
                return log_failure(error);
            }
            return match log.finish() {
                Ok(log_path) => response::prepare_runtime_failure(diagnostic, log_path),
                Err(error) => log_failure(error),
            };
        }
    };

    let dataset = match kat_datasource::publish_materialized_source(
        target.path(),
        kat_datasource::MaterializedSourcePublication {
            pack: pack.name(),
            source: &arguments.source,
            arguments: source_arguments,
            working_directory: &recipe_working_directory,
            table_names: &runtime.tables,
            export_directory: &export_path,
            replace: arguments.replace,
        },
    ) {
        Ok(dataset) => dataset,
        Err(source) => {
            return finish_failure(log, MaterializeOperationError::Dataset { source });
        }
    };
    let path = match unicode_path("Dataset", dataset.path().to_path_buf()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let result = MaterializeResult {
        path,
        pack: pack.name().to_owned(),
        source: arguments.source,
        kind: "materialized",
        tables: runtime.tables,
    };
    if let Err(error) = log.append(b"status: success\n") {
        return log_failure(error);
    }
    match log.finish() {
        Ok(log_path) => response::prepare_success_with_log(result, Some(log_path)),
        Err(error) => log_failure(error),
    }
}

fn normalized_tables(mut tables: Vec<String>) -> Result<Vec<String>, MaterializeOperationError> {
    tables.sort();
    tables.dedup();
    if let Some(name) = tables
        .iter()
        .find(|name| !workflow_runtime::valid_output_name(name))
    {
        return Err(MaterializeOperationError::InvalidTableName { name: name.clone() });
    }
    Ok(tables)
}

fn select_source_arguments(
    explicit: &[String],
    current_directory: &Path,
    binding: Option<&kat_datasource::ResolvedSource>,
) -> Result<(Vec<String>, PathBuf), MaterializeOperationError> {
    if !explicit.is_empty() {
        return Ok((explicit.to_vec(), current_directory.to_path_buf()));
    }
    match binding {
        Some(
            kat_datasource::ResolvedSource::External {
                arguments,
                working_directory,
                ..
            }
            | kat_datasource::ResolvedSource::Materialized {
                arguments,
                working_directory,
                ..
            },
        ) => Ok((arguments.clone(), working_directory.clone())),
        None => Ok((Vec::new(), current_directory.to_path_buf())),
    }
}

fn canonical_current_directory() -> Result<PathBuf, MaterializeOperationError> {
    let path = std::env::current_dir().map_err(MaterializeOperationError::CurrentDirectory)?;
    dunce::canonicalize(&path)
        .map_err(|source| MaterializeOperationError::CanonicalizeCurrentDirectory { path, source })
}

fn unicode_path(label: &'static str, path: PathBuf) -> Result<String, MaterializeOperationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(MaterializeOperationError::NonUnicodePath { label, path })
}

fn finish_failure(
    mut log: OperationLog,
    error: MaterializeOperationError,
) -> response::PreparedResponse<MaterializeResult> {
    let details = project_inline_text(&error.to_string());
    if let Err(log_error) = log.append(format!("status: failure\nerror: {details}\n").as_bytes()) {
        return log_failure(log_error);
    }
    let report = miette::Report::new(error);
    match log.finish() {
        Ok(log_path) => response::prepare_cli_failure_with_log(report, Some(log_path)),
        Err(error) => log_failure(error),
    }
}

fn log_failure(error: OperationLogError) -> response::PreparedResponse<MaterializeResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        MaterializeOperationError::IncompleteOperationLog(error)
    } else {
        MaterializeOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

#[derive(Debug, Error, Diagnostic)]
enum MaterializeOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("Materialize Operation log could not be delivered")]
    #[diagnostic(help(
        "Provide writable KAT Data Home storage and retry the complete Materialize"
    ))]
    OperationLog(#[source] OperationLogError),
    #[error("Materialize Operation log is incomplete")]
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
    #[error("Dataset materialization preparation or publication failed")]
    #[diagnostic(help("Provide a valid Dataset target and Source selection, then retry"))]
    Dataset {
        #[source]
        source: kat_datasource::DatasetMutationError,
    },
    #[error("Dataset already binds {pack}/{source_name} as {kind}; pass --replace to replace it")]
    BindingExists {
        pack: String,
        source_name: String,
        kind: &'static str,
    },
    #[error("invalid Source table name: {name:?}")]
    InvalidTableName { name: String },
    #[error("failed to read the current working directory")]
    CurrentDirectory(#[source] io::Error),
    #[error("failed to resolve the current working directory {path}")]
    CanonicalizeCurrentDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to create the private materialize export directory")]
    CreateExportDirectory {
        #[source]
        source: io::Error,
    },
    #[error("failed to resolve the private materialize export directory")]
    CanonicalizeExportDirectory {
        #[source]
        source: io::Error,
    },
    #[error("{label} path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { label: &'static str, path: PathBuf },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Operation};

    #[test]
    fn parser_accepts_repeated_tables_and_forwards_only_trailing_source_arguments() {
        let cli = Cli::try_parse_from([
            "kat",
            "materialize",
            "--pack",
            "example",
            "--source",
            "logs",
            "--dataset",
            "dataset",
            "--table",
            "events",
            "--table",
            "snapshots",
            "--",
            "--files",
            "capture.log",
        ])
        .unwrap();
        let Operation::Materialize(arguments) = cli.operation else {
            panic!("expected materialize operation");
        };
        assert_eq!(arguments.dataset, PathBuf::from("dataset"));
        assert_eq!(arguments.tables, ["events", "snapshots"]);
        assert_eq!(arguments.source_arguments, ["--files", "capture.log"]);
        assert!(
            Cli::try_parse_from([
                "kat",
                "materialize",
                "--pack",
                "example",
                "--source",
                "logs",
            ])
            .is_err()
        );
    }

    #[test]
    fn table_selection_is_sorted_and_deduplicated() {
        assert_eq!(
            normalized_tables(vec![
                "snapshots".to_owned(),
                "events".to_owned(),
                "snapshots".to_owned(),
            ])
            .unwrap(),
            ["events", "snapshots"]
        );
        assert!(normalized_tables(vec!["bad-name".to_owned()]).is_err());
    }

    #[test]
    fn explicit_arguments_take_priority_over_an_external_binding() {
        let binding = kat_datasource::ResolvedSource::External {
            pack: "example".to_owned(),
            source: "logs".to_owned(),
            arguments: vec!["--old".to_owned()],
            working_directory: PathBuf::from("C:\\old"),
        };
        let selected = select_source_arguments(
            &["--new".to_owned()],
            Path::new("C:\\current"),
            Some(&binding),
        )
        .unwrap();
        assert_eq!(selected.0, ["--new"]);
        assert_eq!(selected.1, PathBuf::from("C:\\current"));
    }

    #[test]
    fn saved_external_arguments_are_replayed_when_no_new_arguments_are_given() {
        let binding = kat_datasource::ResolvedSource::External {
            pack: "example".to_owned(),
            source: "logs".to_owned(),
            arguments: vec!["--saved".to_owned(), "capture.log".to_owned()],
            working_directory: PathBuf::from("C:\\saved"),
        };
        let selected =
            select_source_arguments(&[], Path::new("C:\\current"), Some(&binding)).unwrap();
        assert_eq!(selected.0, ["--saved", "capture.log"]);
        assert_eq!(selected.1, PathBuf::from("C:\\saved"));
    }

    #[test]
    fn an_unbound_source_uses_empty_arguments_from_the_current_directory() {
        let selected = select_source_arguments(&[], Path::new("C:\\current"), None).unwrap();
        assert!(selected.0.is_empty());
        assert_eq!(selected.1, PathBuf::from("C:\\current"));
    }

    #[test]
    fn saved_materialized_arguments_are_replayed_when_no_new_arguments_are_given() {
        let binding = kat_datasource::ResolvedSource::Materialized {
            pack: "example".to_owned(),
            source: "logs".to_owned(),
            arguments: vec!["--saved".to_owned(), "capture.log".to_owned()],
            working_directory: PathBuf::from("C:\\saved"),
            tables: Vec::new(),
        };
        let selected =
            select_source_arguments(&[], Path::new("C:\\current"), Some(&binding)).unwrap();
        assert_eq!(selected.0, ["--saved", "capture.log"]);
        assert_eq!(selected.1, PathBuf::from("C:\\saved"));
    }
}
