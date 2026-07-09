use std::{io::Write, path::PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

#[derive(Clone, Debug, Parser)]
#[command(name = "kat-rs")]
#[command(about = "Run short-lived kat-rs dataset and Pack commands")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Dataset(DatasetCommand),
    Pack(PackCommand),
    Version,
}

#[derive(Clone, Debug, Args)]
pub struct DatasetCommand {
    #[command(subcommand)]
    pub command: DatasetSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DatasetSubcommand {
    Materialize(DatasetMaterializeCommand),
    Inspect(DatasetPathArgs),
    Query(DatasetQueryArgs),
}

#[derive(Clone, Debug, Args)]
pub struct DatasetMaterializeCommand {
    #[command(subcommand)]
    pub source: DatasetMaterializeSource,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DatasetMaterializeSource {
    Sqlite(SqliteMaterializeArgs),
}

#[derive(Clone, Debug, Args)]
pub struct SqliteMaterializeArgs {
    pub db_path: PathBuf,
    pub dataset_path: PathBuf,
}

#[derive(Clone, Debug, Args)]
pub struct DatasetPathArgs {
    pub dataset: PathBuf,
}

#[derive(Clone, Debug, Args)]
pub struct DatasetQueryArgs {
    pub dataset: PathBuf,
    #[arg(long)]
    pub sql: String,
}

#[derive(Clone, Debug, Args)]
pub struct PackCommand {
    #[command(subcommand)]
    pub command: PackSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum PackSubcommand {
    Inspect(PackInspectArgs),
    Run(PackRunArgs),
}

#[derive(Clone, Debug, Args)]
pub struct PackInspectArgs {
    pub pack_root: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct PackRunArgs {
    pub pack_root: PathBuf,
    pub workflow: String,
    #[arg(long)]
    pub dataset: PathBuf,
    #[arg(long)]
    pub run_dir: PathBuf,
    #[arg(long = "param")]
    pub params: Vec<String>,
}

pub async fn run(cli: Cli, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match run_inner(cli, out).await {
        Ok(()) => 0,
        Err(error) => {
            log::error!("command failed: {error:#}");
            let _ = writeln!(err, "{error:#}");
            1
        }
    }
}

async fn run_inner(cli: Cli, out: &mut dyn Write) -> anyhow::Result<()> {
    match cli.command {
        Command::Dataset(args) => run_dataset(args, out).await,
        Command::Pack(args) => run_pack(args, out).await,
        Command::Version => run_version(out),
    }
}

async fn run_dataset(args: DatasetCommand, out: &mut dyn Write) -> anyhow::Result<()> {
    match args.command {
        DatasetSubcommand::Materialize(args) => match args.source {
            DatasetMaterializeSource::Sqlite(args) => {
                kat_rs_datasource::materialize_sqlite_dataset(args.db_path, args.dataset_path).await
            }
        },
        DatasetSubcommand::Inspect(args) => {
            let tables = kat_rs_datasource::inspect_dataset_tables(&args.dataset)?;
            writeln!(out, "name\tkind\tsize_bytes\tpath")?;
            for table in tables {
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}",
                    table.name, table.kind, table.size_bytes, table.path
                )?;
            }
            Ok(())
        }
        DatasetSubcommand::Query(args) => {
            let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&args.dataset)
                .await
                .with_context(|| format!("failed to open dataset {}", args.dataset.display()))?;
            let rows = datasource.query_json(&args.sql).await?;
            serde_json::to_writer_pretty(&mut *out, &rows)?;
            writeln!(out)?;
            Ok(())
        }
    }
}

async fn run_pack(args: PackCommand, out: &mut dyn Write) -> anyhow::Result<()> {
    match args.command {
        PackSubcommand::Inspect(args) => {
            let stdout = crate::python_worker::run_discovery(&args.pack_root)?;
            if args.json {
                write!(out, "{stdout}")?;
            } else {
                let value: serde_json::Value = serde_json::from_str(&stdout)?;
                print_pack_inspect_text(out, &value)?;
            }
            Ok(())
        }
        PackSubcommand::Run(args) => {
            let request = crate::python_worker::PackRunRequest {
                pack_root: args.pack_root,
                workflow: args.workflow,
                dataset_path: args.dataset,
                run_dir: args.run_dir,
                inputs: crate::python_worker::parse_params(&args.params)?,
            };
            let stdout = crate::python_worker::run_pack(&request)?;
            writeln!(out, "{stdout}")?;
            Ok(())
        }
    }
}

fn print_pack_inspect_text(
    out: &mut dyn Write,
    manifest: &serde_json::Value,
) -> anyhow::Result<()> {
    for section in ["workflows", "facts", "computes"] {
        writeln!(out, "{section}:")?;
        if let Some(items) = manifest[section].as_array() {
            for item in items {
                writeln!(
                    out,
                    "  {} - {}",
                    item["name"].as_str().unwrap_or("<unknown>"),
                    item["title"].as_str().unwrap_or("")
                )?;
            }
        }
    }
    Ok(())
}

fn run_version(out: &mut dyn Write) -> anyhow::Result<()> {
    writeln!(out, "{}", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
