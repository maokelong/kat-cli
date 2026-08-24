use std::{io, path::PathBuf};

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
pub(super) struct BindArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Source name from the PACK.
    #[arg(long, value_name = "NAME")]
    source: String,
    /// Create or update this KAT Dataset directory.
    #[arg(long, value_name = "DIRECTORY")]
    dataset: PathBuf,
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
    /// The Binding and Operation log retain the complete vector.
    #[arg(last = true, value_name = "ARGUMENT")]
    source_arguments: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct BindResult {
    path: String,
    pack: String,
    source: String,
    kind: &'static str,
}

pub(super) fn execute(arguments: BindArgs) -> response::PreparedResponse<BindResult> {
    let data_home = match locate_data_home() {
        Ok(path) => path,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    let mut log = match OperationLog::create(&data_home, "bind", |file| {
        writeln!(
            file,
            "operation: kat bind\npack: {}\nsource: {}\ndataset: {:?}\narguments: {:?}",
            project_inline_text(&arguments.pack),
            project_inline_text(&arguments.source),
            arguments.dataset,
            arguments.source_arguments,
        )
    }) {
        Ok(log) => log,
        Err(error) => return log_failure(error),
    };

    let skill_root = match locate_skill_root() {
        Ok(path) => path,
        Err(source) => return finish_failure(log, BindOperationError::SkillRoot(source)),
    };
    let discovered = match pack_discovery::discover(PackDiscoveryPaths {
        skill_pack_search_directory: skill_root.join("assets").join("packs"),
        data_home_pack_search_directory: data_home.join("packs"),
        additional_pack_directories: arguments.pack_directories,
    }) {
        Ok(packs) => packs,
        Err(source) => return finish_failure(log, BindOperationError::Discovery { source }),
    };
    let Some(pack) = discovered.get(&arguments.pack) else {
        return finish_failure(
            log,
            BindOperationError::UnknownPack {
                name: arguments.pack,
            },
        );
    };
    let pack_path = match unicode_path("PACK", pack.directory().to_path_buf()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };

    let target = match kat_datasource::inspect_dataset_target(
        &arguments.dataset,
        pack.name(),
        &arguments.source,
    ) {
        Ok(target) => target,
        Err(source) => return finish_failure(log, BindOperationError::Dataset { source }),
    };
    if let Some(kind) = target.binding()
        && !arguments.replace
    {
        return finish_failure(
            log,
            BindOperationError::BindingExists {
                pack: pack.name().to_owned(),
                source_name: arguments.source,
                kind: kind.as_str(),
            },
        );
    }

    let working_directory = match canonical_current_directory() {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let argument_base = match unicode_path("current working directory", working_directory.clone()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let invocation = workflow_runtime::BindSourceInvocation {
        pack_name: pack.name().to_owned(),
        pack_path,
        source_name: arguments.source.clone(),
        arguments: arguments.source_arguments.clone(),
        argument_base,
    };
    let outcome = match workflow_runtime::bind_source(log, invocation) {
        Ok(outcome) => outcome,
        Err(error) => {
            let log_path = error.log_path();
            return response::prepare_cli_failure_with_log(miette::Report::new(error), log_path);
        }
    };
    log = match outcome {
        workflow_runtime::SourceRuntimeOutcome::Success { log, .. } => log,
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

    let dataset = match kat_datasource::write_external_binding(
        target.path(),
        pack.name(),
        &arguments.source,
        arguments.source_arguments,
        &working_directory,
        arguments.replace,
    ) {
        Ok(dataset) => dataset,
        Err(source) => return finish_failure(log, BindOperationError::Dataset { source }),
    };
    let path = match unicode_path("Dataset", dataset.path().to_path_buf()) {
        Ok(path) => path,
        Err(error) => return finish_failure(log, error),
    };
    let result = BindResult {
        path,
        pack: pack.name().to_owned(),
        source: arguments.source,
        kind: "external",
    };
    if let Err(error) = log.append(b"status: success\n") {
        return log_failure(error);
    }
    match log.finish() {
        Ok(log_path) => response::prepare_success_with_log(result, Some(log_path)),
        Err(error) => log_failure(error),
    }
}

fn canonical_current_directory() -> Result<PathBuf, BindOperationError> {
    let path = std::env::current_dir().map_err(BindOperationError::CurrentDirectory)?;
    dunce::canonicalize(&path)
        .map_err(|source| BindOperationError::CanonicalizeCurrentDirectory { path, source })
}

fn unicode_path(label: &'static str, path: PathBuf) -> Result<String, BindOperationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(BindOperationError::NonUnicodePath { label, path })
}

fn finish_failure(
    mut log: OperationLog,
    error: BindOperationError,
) -> response::PreparedResponse<BindResult> {
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

fn log_failure(error: OperationLogError) -> response::PreparedResponse<BindResult> {
    let log_path = error.readable_path();
    let error = if log_path.is_some() {
        BindOperationError::IncompleteOperationLog(error)
    } else {
        BindOperationError::OperationLog(error)
    };
    response::prepare_cli_failure_with_log(miette::Report::new(error), log_path)
}

#[derive(Debug, Error, Diagnostic)]
enum BindOperationError {
    #[error("KAT Skill is unavailable")]
    #[diagnostic(help("Run the kat executable from a complete KAT Skill deployment"))]
    SkillRoot(#[source] SkillRootError),
    #[error("Bind Operation log could not be delivered")]
    #[diagnostic(help("Provide writable KAT Data Home storage and retry the complete Bind"))]
    OperationLog(#[source] OperationLogError),
    #[error("Bind Operation log is incomplete")]
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
    #[error("Dataset Binding preparation failed")]
    #[diagnostic(help("Provide a valid Dataset target, PACK, Source, and Binding configuration"))]
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
    #[error("failed to read the current working directory")]
    CurrentDirectory(#[source] io::Error),
    #[error("failed to resolve the current working directory {path}")]
    CanonicalizeCurrentDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{label} path cannot be represented as native Unicode: {path:?}")]
    NonUnicodePath { label: &'static str, path: PathBuf },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::{Cli, Operation};

    #[test]
    fn parser_forwards_source_arguments_only_after_separator() {
        let cli = Cli::try_parse_from([
            "kat",
            "bind",
            "--pack",
            "example",
            "--source",
            "logs",
            "--dataset",
            "dataset",
            "--",
            "--files",
            "capture.log",
        ])
        .unwrap();
        let Operation::Bind(arguments) = cli.operation else {
            panic!("expected bind operation");
        };
        assert_eq!(arguments.dataset, PathBuf::from("dataset"));
        assert_eq!(arguments.source_arguments, ["--files", "capture.log"]);
        assert!(
            Cli::try_parse_from([
                "kat",
                "bind",
                "--pack",
                "example",
                "--source",
                "logs",
                "--dataset",
                "dataset",
                "--files",
                "capture.log",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["kat", "bind", "--pack", "example", "--source", "logs",]).is_err()
        );
    }
}
