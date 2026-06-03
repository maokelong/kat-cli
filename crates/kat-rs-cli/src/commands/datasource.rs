use anyhow::Result;
use clap::{Args, Subcommand};
use kat_rs_datasource::{
    infer_required_tables, DatasetInput, DatasourceQueryRequest, HtraceDatasource, TraceSource,
};
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct DatasourceCommand {
    #[command(subcommand)]
    pub command: DatasourceSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DatasourceSubcommand {
    Query(QueryArgs),
    Validate(ValidateArgs),
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
    #[arg(long)]
    pub sql: String,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
}

pub async fn run(command: DatasourceCommand) -> Result<()> {
    match command.command {
        DatasourceSubcommand::Query(args) => query(args).await,
        DatasourceSubcommand::Validate(args) => validate(args).await,
    }
}

fn dataset_input_from_traces(traces: Vec<PathBuf>, required_tables: Vec<String>) -> DatasetInput {
    DatasetInput {
        sources: traces
            .into_iter()
            .map(|path| TraceSource {
                path,
                format_hint: None,
                source_name: None,
            })
            .collect(),
        required_tables,
    }
}

async fn query(args: QueryArgs) -> Result<()> {
    let datasource = HtraceDatasource::new();
    let required_tables = infer_required_tables(&args.sql);
    let handle = datasource
        .open_dataset(dataset_input_from_traces(
            args.traces,
            required_tables.clone(),
        ))
        .await?;
    let mut request = DatasourceQueryRequest::new(args.sql);
    request.required_tables = required_tables;
    let result = datasource.query(&handle, request).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn validate(args: ValidateArgs) -> Result<()> {
    let datasource = HtraceDatasource::new();
    let handle = datasource
        .open_dataset(dataset_input_from_traces(args.traces, Vec::new()))
        .await?;
    let inspection = datasource.inspect(&handle).await?;
    let report = json!({
        "status": "ok",
        "dataset_id": inspection.dataset_id,
        "source_count": inspection.source_count,
        "table_count": inspection.tables.len(),
        "diagnostics": [],
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
