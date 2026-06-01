use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use htrace_core::{OpenOptions, QueryRequest, TraceInput, TraceQueryEngine};
use htrace_query::HtraceDataFusionEngine;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "htrace-engine")]
#[command(about = "Rust/DataFusion query engine for OpenHarmony trace files")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Query {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        sql: Option<String>,
        #[arg(long)]
        sql_file: Option<PathBuf>,
        #[arg(long, default_value_t = 10_000)]
        max_inline_rows: usize,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let engine = HtraceDataFusionEngine::new();

    match cli.command {
        Command::Inspect {
            trace,
            cache_dir,
            json,
        } => {
            let handle = engine
                .open(TraceInput { path: trace }, OpenOptions { cache_dir })
                .await?;
            let inspection = engine.inspect(&handle).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&inspection)?);
            } else {
                println!("trace_id: {}", inspection.trace_id);
                println!("schema_version: {}", inspection.schema_version);
                println!("clock_domain: {}", inspection.clock_domain);
                println!("start_ts: {:?}", inspection.start_ts);
                println!("end_ts: {:?}", inspection.end_ts);
                for (name, table) in inspection.tables {
                    println!("{name}: {} rows", table.row_count);
                }
            }
            engine.close(handle).await?;
        }
        Command::Query {
            trace,
            sql,
            sql_file,
            max_inline_rows,
            cache_dir,
            json,
        } => {
            let sql = match (sql, sql_file) {
                (Some(sql), None) => sql,
                (None, Some(path)) => fs::read_to_string(&path)
                    .with_context(|| format!("failed to read SQL file {}", path.display()))?,
                (Some(_), Some(_)) => anyhow::bail!("use either --sql or --sql-file, not both"),
                (None, None) => anyhow::bail!("missing --sql or --sql-file"),
            };

            let handle = engine
                .open(TraceInput { path: trace }, OpenOptions { cache_dir })
                .await?;
            let result = engine
                .query(
                    &handle,
                    QueryRequest {
                        sql,
                        max_inline_rows,
                    },
                )
                .await?;

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&result.rows)?);
                eprintln!(
                    "status={}, rows={}, truncated={}",
                    result.status, result.stats.rows_returned, result.stats.truncated
                );
            }
            engine.close(handle).await?;
        }
    }

    Ok(())
}
