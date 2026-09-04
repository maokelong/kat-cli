use std::{path::PathBuf, process::ExitCode};

use clap::{ArgGroup, Args, Subcommand};

use crate::response;

#[derive(Args)]
pub(super) struct InspectArgs {
    #[command(subcommand)]
    target: Option<InspectTarget>,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order; results remain sorted by PACK name."
    )]
    pack_directories: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum InspectTarget {
    /// Inspect the Workflows declared by one PACK or the Workflow used by one Run.
    Workflow(InspectWorkflowArgs),
    /// Inspect the Providers declared by one PACK.
    Provider(InspectProviderArgs),
    /// Inspect the published Run inventory of one Analysis Session.
    Session(InspectSessionArgs),
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("source")
        .required(true)
        .multiple(false)
        .args(["pack", "run"])
))]
struct InspectWorkflowArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME", conflicts_with = "run")]
    pack: Option<String>,
    /// Select one exact Workflow from --pack.
    #[arg(long, value_name = "NAME", requires = "pack", conflicts_with = "run")]
    workflow: Option<String>,
    /// Select the current Workflow declaration used by one published Run.
    #[arg(
        long,
        value_name = "RUN_ID",
        requires = "session",
        conflicts_with_all = ["pack", "workflow"]
    )]
    run: Option<String>,
    /// Select the Analysis Session containing --run.
    #[arg(
        long,
        value_name = "SESSION_ID",
        requires = "run",
        conflicts_with = "pack"
    )]
    session: Option<String>,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order."
    )]
    pack_directories: Vec<PathBuf>,
}

#[derive(Args)]
struct InspectProviderArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Provider from --pack.
    #[arg(long, value_name = "NAME")]
    provider: Option<String>,
    #[arg(
        long = "pack-dir",
        value_name = "DIRECTORY",
        help = "Add an exact PACK candidate directory containing pack.toml. Repetition preserves validation order."
    )]
    pack_directories: Vec<PathBuf>,
}

#[derive(Args)]
struct InspectSessionArgs {
    /// Select one exact published Analysis Session ID.
    #[arg(long, value_name = "SESSION_ID")]
    session: String,
}

pub(super) fn execute(arguments: InspectArgs) -> ExitCode {
    if let Err(error) = validate_arguments(&arguments) {
        return response::publish(response::prepare_cli_failure::<()>(error));
    }
    match arguments.target {
        None => {
            let prepared = match super::inspect_packs(arguments.pack_directories) {
                Ok(result) => response::prepare_success(result),
                Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
            };
            response::publish(prepared)
        }
        Some(InspectTarget::Workflow(InspectWorkflowArgs {
            pack: Some(pack),
            workflow,
            run: None,
            session: None,
            pack_directories,
        })) => response::publish(super::inspect_target_pack(
            pack,
            joined_pack_directories(arguments.pack_directories, pack_directories),
            super::InspectKnowledgeTarget::Workflow(workflow),
        )),
        Some(InspectTarget::Provider(InspectProviderArgs {
            pack,
            provider,
            pack_directories,
        })) => response::publish(super::inspect_target_pack(
            pack,
            joined_pack_directories(arguments.pack_directories, pack_directories),
            super::InspectKnowledgeTarget::Provider(provider),
        )),
        Some(InspectTarget::Session(InspectSessionArgs { session })) => {
            response::publish(super::inspect_session(session))
        }
        Some(InspectTarget::Workflow(InspectWorkflowArgs {
            pack: None,
            run: Some(run),
            session: Some(session),
            pack_directories,
            workflow: _,
        })) => response::publish(super::inspect_run_workflow(
            session,
            run,
            joined_pack_directories(arguments.pack_directories, pack_directories),
        )),
        Some(InspectTarget::Workflow(_)) => {
            unreachable!("clap guarantees exactly one Workflow inspection source")
        }
    }
}

fn joined_pack_directories(
    mut before_subcommand: Vec<PathBuf>,
    after_subcommand: Vec<PathBuf>,
) -> Vec<PathBuf> {
    before_subcommand.extend(after_subcommand);
    before_subcommand
}

fn validate_arguments(arguments: &InspectArgs) -> Result<(), miette::Report> {
    if matches!(&arguments.target, Some(InspectTarget::Session(_)))
        && !arguments.pack_directories.is_empty()
    {
        return Err(miette::miette!(
            "--pack-dir cannot be used with `kat inspect session`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::validate_arguments;
    use crate::{Cli, Operation};

    const SESSION_ID: &str = "019f6e00-0000-7000-8000-000000000060";

    #[test]
    fn pack_directories_remain_accepted_before_and_after_pack_targets() {
        assert!(
            Cli::try_parse_from([
                "kat",
                "inspect",
                "--pack-dir",
                "before",
                "workflow",
                "--pack",
                "example",
                "--pack-dir",
                "after",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "kat",
                "inspect",
                "provider",
                "--pack",
                "example",
                "--pack-dir",
                "after",
            ])
            .is_ok()
        );
    }

    #[test]
    fn session_inspection_neither_accepts_nor_offers_pack_directories() {
        let parsed = Cli::try_parse_from([
            "kat",
            "inspect",
            "--pack-dir",
            "unused",
            "session",
            "--session",
            SESSION_ID,
        ])
        .unwrap();
        let Operation::Inspect(arguments) = parsed.operation else {
            panic!("expected inspect operation");
        };
        assert!(validate_arguments(&arguments).is_err());
        assert!(
            Cli::try_parse_from([
                "kat",
                "inspect",
                "session",
                "--session",
                SESSION_ID,
                "--pack-dir",
                "unused",
            ])
            .is_err()
        );

        let mut command = Cli::command();
        let session = command
            .find_subcommand_mut("inspect")
            .and_then(|inspect| inspect.find_subcommand_mut("session"))
            .expect("inspect session command exists");
        let help = session.render_long_help().to_string();
        assert!(!help.contains("--pack-dir"));
    }
}
