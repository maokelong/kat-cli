use anyhow::Result;
use clap::{Args, Subcommand};
use kat_rs_datasource::{
    infer_required_tables, run_golden_suite, DatasetInput, DatasourceQueryRequest,
    DatasourceService, HtraceDatasource, QueryOutputMode, TraceSource,
};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct DatasourceCommand {
    #[command(subcommand)]
    pub command: DatasourceSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DatasourceSubcommand {
    Inspect(InspectArgs),
    Query(QueryArgs),
    Validate(ValidateArgs),
    Bench(BenchArgs),
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
    #[arg(long)]
    pub sql: Option<String>,
    #[arg(long)]
    pub sql_file: Option<PathBuf>,
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 10_000)]
    pub max_rows_inline: usize,
    #[arg(long, default_value_t = 1_048_576)]
    pub max_result_bytes_inline: usize,
    #[arg(long, value_parser = ["inline-json", "artifact"], default_value = "inline-json")]
    pub output: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
    #[arg(long)]
    pub query_suite: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct BenchArgs {
    #[arg(long = "trace", required = true)]
    pub traces: Vec<PathBuf>,
    #[arg(long)]
    pub sql: String,
    #[arg(long)]
    pub json: bool,
}

pub async fn run(command: DatasourceCommand) -> Result<()> {
    match command.command {
        DatasourceSubcommand::Inspect(args) => inspect(args).await,
        DatasourceSubcommand::Query(args) => query(args).await,
        DatasourceSubcommand::Validate(args) => validate(args).await,
        DatasourceSubcommand::Bench(args) => bench(args).await,
    }
}

fn datasource_service() -> DatasourceService<HtraceDatasource> {
    DatasourceService::new(HtraceDatasource::new())
}

fn dataset_input_from_traces(
    traces: Vec<PathBuf>,
    cache_dir: Option<PathBuf>,
    required_tables: Vec<String>,
) -> DatasetInput {
    DatasetInput {
        sources: traces
            .into_iter()
            .map(|path| TraceSource {
                path,
                format_hint: None,
                source_name: None,
            })
            .collect(),
        cache_dir,
        required_tables,
    }
}

async fn inspect(args: InspectArgs) -> Result<()> {
    let datasource = datasource_service();
    let handle = datasource
        .open_dataset(dataset_input_from_traces(
            args.traces,
            args.cache_dir,
            Vec::new(),
        ))
        .await?;
    let inspection = datasource.inspect(&handle).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&inspection)?);
    } else {
        println!("dataset_id: {}", inspection.dataset_id);
        println!("schema_version: {}", inspection.schema_version);
        println!("source_count: {}", inspection.source_count);
        for (table, capability) in inspection.tables {
            println!("{table}: {} rows", capability.row_count);
        }
    }
    Ok(())
}

async fn validate(args: ValidateArgs) -> Result<()> {
    let datasource = datasource_service();
    let handle = datasource
        .open_dataset(dataset_input_from_traces(args.traces, None, Vec::new()))
        .await?;
    let report = run_golden_suite(&datasource, &handle, &args.query_suite).await?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("validation status: {}", report.status);
    }

    if report.status == "ok" {
        Ok(())
    } else {
        anyhow::bail!("golden validation failed")
    }
}

async fn bench(args: BenchArgs) -> Result<()> {
    let datasource = datasource_service();
    let required_tables = infer_required_tables(&args.sql);
    let open_started = std::time::Instant::now();
    let handle = datasource
        .open_dataset(dataset_input_from_traces(
            args.traces,
            None,
            required_tables,
        ))
        .await?;
    let open_elapsed_ms = open_started.elapsed().as_millis() as u64;

    let result = datasource
        .query(&handle, DatasourceQueryRequest::new(args.sql))
        .await?;
    let report = serde_json::json!({
        "open_elapsed_ms": open_elapsed_ms,
        "query_elapsed_ms": result.metrics.elapsed_ms,
        "phase_elapsed_ms": result.metrics.phase_elapsed_ms,
        "rows_returned": result.stats.rows_returned,
        "bytes_inline": result.stats.bytes_inline,
        "cache_hit": result.metrics.cache_hit
    });

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "open={}ms query={}ms rows={} bytes={} cache_hit={}",
            report["open_elapsed_ms"],
            report["query_elapsed_ms"],
            report["rows_returned"],
            report["bytes_inline"],
            report["cache_hit"]
        );
    }
    Ok(())
}

async fn query(args: QueryArgs) -> Result<()> {
    let sql = match (args.sql, args.sql_file) {
        (Some(sql), None) => sql,
        (None, Some(path)) => fs::read_to_string(&path)?,
        (Some(_), Some(_)) => anyhow::bail!("use either --sql or --sql-file, not both"),
        (None, None) => anyhow::bail!("missing --sql or --sql-file"),
    };

    let datasource = datasource_service();
    let required_tables = infer_required_tables(&sql);
    let handle = datasource
        .open_dataset(dataset_input_from_traces(
            args.traces,
            args.cache_dir,
            required_tables.clone(),
        ))
        .await?;
    let mut request = DatasourceQueryRequest::new(sql);
    request.required_tables = required_tables;
    request.limits.max_rows_inline = args.max_rows_inline;
    request.limits.max_result_bytes_inline = args.max_result_bytes_inline;
    request.output = match args.output.as_str() {
        "artifact" => QueryOutputMode::Artifact,
        _ => QueryOutputMode::InlineJson,
    };
    let result = datasource.query(&handle, request).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result.rows)?);
    }
    Ok(())
}
