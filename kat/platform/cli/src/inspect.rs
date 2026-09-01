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
        global = true,
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
    #[arg(long, value_name = "RUN_ID", conflicts_with_all = ["pack", "workflow"])]
    run: Option<String>,
}

#[derive(Args)]
struct InspectProviderArgs {
    /// Select one exact PACK by manifest name.
    #[arg(long, value_name = "NAME")]
    pack: String,
    /// Select one exact Provider from --pack.
    #[arg(long, value_name = "NAME")]
    provider: Option<String>,
}

pub(super) fn execute(arguments: InspectArgs) -> ExitCode {
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
        })) => response::publish(super::inspect_target_pack(
            pack,
            arguments.pack_directories,
            super::InspectKnowledgeTarget::Workflow(workflow),
        )),
        Some(InspectTarget::Provider(InspectProviderArgs { pack, provider })) => {
            response::publish(super::inspect_target_pack(
                pack,
                arguments.pack_directories,
                super::InspectKnowledgeTarget::Provider(provider),
            ))
        }
        Some(InspectTarget::Workflow(InspectWorkflowArgs {
            pack: None,
            run: Some(run),
            ..
        })) => response::publish(super::inspect_run_workflow(run, arguments.pack_directories)),
        Some(InspectTarget::Workflow(_)) => {
            unreachable!("clap guarantees exactly one Workflow inspection source")
        }
    }
}
