use std::{io::Write, path::PathBuf};

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
    Probe(ProbeArgs),
    Query(QueryArgs),
}

#[derive(Clone, Debug, Args)]
pub struct ProbeArgs {
    #[command(subcommand)]
    pub command: ProbeCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ProbeCommand {
    Run(ProbeRunArgs),
}

impl ProbeCommand {
    pub fn probe_id(&self) -> &str {
        match self {
            ProbeCommand::Run(args) => &args.probe,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct ProbeRunArgs {
    #[arg(long = "probe")]
    pub probe: String,
    #[arg(long, value_enum)]
    pub source: crate::trace_runtime::ProbeSourceArg,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long = "params-file")]
    pub params_file: PathBuf,
    #[arg(long = "run-dir")]
    pub run_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub struct QueryArgs {
    #[arg(long, value_enum)]
    pub source: SourceArg,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long)]
    pub sql: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SourceArg {
    Hitrace,
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
        Command::Probe(args) => run_probe(args, out).await,
        Command::Query(args) => run_query(args, out).await,
    }
}

async fn run_probe(args: ProbeArgs, out: &mut dyn Write) -> Result<(), CommandError> {
    match args.command {
        ProbeCommand::Run(args) => crate::trace_runtime::run_probe(
            crate::trace_runtime::ProbeRunOptions {
                probe: args.probe,
                source: args.source,
                file: args.file,
                params_file: args.params_file,
                run_dir: args.run_dir,
            },
            out,
        )
        .await
        .map_err(CommandError::from_runtime),
    }
}

async fn run_query(args: QueryArgs, out: &mut dyn Write) -> Result<(), CommandError> {
    let datasource = match args.source {
        SourceArg::Hitrace => kat_rs_datasource::TraceDatasource::from_hitrace(args.file),
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

enum CommandError {
    Runtime(anyhow::Error),
}

impl CommandError {
    fn from_runtime(error: impl Into<anyhow::Error>) -> Self {
        Self::Runtime(error.into())
    }
}
