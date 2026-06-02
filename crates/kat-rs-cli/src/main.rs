use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Debug, Parser)]
#[command(name = "kat-rs")]
#[command(about = "kat-rs command line interface")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Datasource(commands::datasource::DatasourceCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Datasource(command) => commands::datasource::run(command).await,
    }
}
