use crate::run::model::{AdvanceTarget, StageId};
use crate::run::{advance_run, go_run, guard_run, init_run, status_run, validate_run};
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct RunCommand {
    #[command(subcommand)]
    pub action: RunAction,
}

#[derive(Debug, Subcommand)]
pub enum RunAction {
    Init {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        trace: String,
        #[arg(long)]
        question: String,
        #[arg(long)]
        domain: Option<String>,
        #[arg(long = "target-process")]
        target_process: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Status {
        run_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Go {
        run_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Validate {
        run_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Guard {
        run_dir: PathBuf,
        #[arg(long)]
        action: String,
        #[arg(long)]
        json: bool,
    },
    Advance {
        run_dir: PathBuf,
        #[arg(long)]
        from: StageId,
        #[arg(long)]
        to: AdvanceTarget,
        #[arg(long = "artifact")]
        artifacts: Vec<PathBuf>,
        #[arg(long)]
        decision: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: RunCommand) -> Result<()> {
    match cmd.action {
        RunAction::Init {
            out,
            trace,
            question,
            domain,
            target_process,
            json,
        } => {
            let summary = init_run(&out, &trace, &question, domain, target_process)?;
            print_summary(&summary, json)?;
        }
        RunAction::Status { run_dir, json } => {
            let summary = status_run(&run_dir)?;
            print_summary(&summary, json)?;
        }
        RunAction::Go { run_dir, json } => {
            let summary = go_run(&run_dir)?;
            print_summary(&summary, json)?;
        }
        RunAction::Validate { run_dir, json } => {
            let summary = validate_run(&run_dir);
            print_summary(&summary, json)?;
        }
        RunAction::Guard {
            run_dir,
            action,
            json,
        } => {
            let summary = guard_run(&run_dir, &action)?;
            print_summary(&summary, json)?;
            if !summary.allowed {
                bail!(
                    "{}",
                    summary
                        .reason
                        .as_deref()
                        .unwrap_or("当前阶段不允许执行该动作")
                );
            }
        }
        RunAction::Advance {
            run_dir,
            from,
            to,
            artifacts,
            decision,
            json,
        } => {
            let artifacts = artifacts
                .into_iter()
                .map(|path| path.display().to_string())
                .collect();
            let summary = advance_run(&run_dir, from, to, artifacts, decision)?;
            print_summary(&summary, json)?;
        }
    }

    Ok(())
}

fn print_summary<T>(summary: &T, json: bool) -> Result<()>
where
    T: serde::Serialize + std::fmt::Debug,
{
    if json {
        println!("{}", serde_json::to_string(summary)?);
    } else {
        println!("{summary:#?}");
    }
    Ok(())
}
