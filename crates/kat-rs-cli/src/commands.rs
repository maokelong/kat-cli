use std::{io::Write, path::PathBuf};

use anyhow::anyhow;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
#[command(name = "kat-rs")]
#[command(about = "Query trace and log files with SQL")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Query(QueryArgs),
}

#[derive(Clone, Debug, Args)]
pub struct QueryArgs {
    #[arg(long, value_enum)]
    pub source: SourceArg,
    #[arg(
        long,
        required_if_eq("source", "hitrace"),
        conflicts_with_all = ["observations_file", "traces_file"]
    )]
    pub file: Option<PathBuf>,
    #[arg(long, required_if_eq("source", "langfuse"), conflicts_with = "file")]
    pub observations_file: Option<PathBuf>,
    #[arg(long, required_if_eq("source", "langfuse"), conflicts_with = "file")]
    pub traces_file: Option<PathBuf>,
    #[arg(long)]
    pub sql: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SourceArg {
    Hitrace,
    Langfuse,
}

impl SourceArg {
    fn as_str(self) -> &'static str {
        match self {
            SourceArg::Hitrace => "hitrace",
            SourceArg::Langfuse => "langfuse",
        }
    }
}

pub async fn run(cli: Cli, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match run_inner(cli, out).await {
        Ok(()) => 0,
        Err(CommandError::Runtime(error)) => {
            log::error!("command failed: {error:#}");
            let _ = writeln!(err, "{error:#}");
            1
        }
    }
}

async fn run_inner(cli: Cli, out: &mut dyn Write) -> Result<(), CommandError> {
    match cli.command {
        Command::Query(args) => run_query(args, out).await,
    }
}

async fn run_query(args: QueryArgs, out: &mut dyn Write) -> Result<(), CommandError> {
    let datasource = match args.source {
        SourceArg::Hitrace => {
            let file = required_path_arg(args.file, "--file", args.source)?;
            kat_rs_datasource::TraceDatasource::from_hitrace(file)
        }
        SourceArg::Langfuse => {
            let observations_file =
                required_path_arg(args.observations_file, "--observations-file", args.source)?;
            let traces_file = required_path_arg(args.traces_file, "--traces-file", args.source)?;
            kat_rs_datasource::TraceDatasource::from_langfuse_legacy(observations_file, traces_file)
                .await
        }
    }
    .map_err(CommandError::from_runtime)?;

    let rows = datasource
        .query_json(&args.sql)
        .await
        .map_err(CommandError::from_runtime)?;

    serde_json::to_writer(&mut *out, &rows).map_err(CommandError::from_runtime)?;
    writeln!(out).map_err(CommandError::from_runtime)?;
    Ok(())
}

fn required_path_arg(
    value: Option<PathBuf>,
    name: &'static str,
    source: SourceArg,
) -> Result<PathBuf, CommandError> {
    value.ok_or_else(|| {
        CommandError::from_runtime(anyhow!(
            "{name} is required when --source {}",
            source.as_str()
        ))
    })
}

enum CommandError {
    Runtime(anyhow::Error),
}

impl CommandError {
    fn from_runtime(error: impl Into<anyhow::Error>) -> Self {
        Self::Runtime(error.into())
    }
}
