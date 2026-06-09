use std::{io::Write, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Parser, Serialize)]
#[command(name = "kat-rs")]
#[command(about = "Query trace and log files with SQL")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Deserialize, Serialize, Subcommand)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Query(QueryArgs),
}

#[derive(Clone, Debug, Deserialize, Serialize, Args)]
pub struct QueryArgs {
    #[arg(long, value_enum)]
    pub source: SourceArg,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long)]
    pub sql: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SourceArg {
    Hitrace,
}

impl From<SourceArg> for kat_rs_datasource::DataSourceType {
    fn from(value: SourceArg) -> Self {
        match value {
            SourceArg::Hitrace => Self::Hitrace,
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
    let mut session = kat_rs_session::Session::create();

    session
        .build_datasource(kat_rs_datasource::DataSourceConfig::new(
            kat_rs_datasource::DataSourceType::from(args.source),
            args.file,
        ))
        .map_err(CommandError::from_runtime)?;

    let rows = session
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
