use crate::config::SkillRoot;
use crate::engine::mock::MockTraceQueryEngine;
use crate::engine::perfetto_shell::PerfettoShellEngine;
use crate::engine::TraceQueryEngine;
use crate::executor::params::{parse_params, prepare_sql};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct AtomicCommand {
    #[command(subcommand)]
    pub action: AtomicAction,
}

#[derive(Debug, Subcommand)]
pub enum AtomicAction {
    List {
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long)]
        domain: Option<String>,
    },
    Run {
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long, default_value = "perfetto")]
        engine: String,
        id: String,
        #[arg(long)]
        trace: PathBuf,
        #[arg(long = "param")]
        params: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: AtomicCommand) -> Result<()> {
    match cmd.action {
        AtomicAction::List { skill_root, domain } => {
            let skill = SkillRoot::load(skill_root)?;
            for atomic in skill.atomics() {
                if domain
                    .as_deref()
                    .map_or(true, |domain| domain == atomic.domain)
                {
                    println!("{}\t{}\t{}", atomic.id, atomic.domain, atomic.description);
                }
            }
        }
        AtomicAction::Run {
            skill_root,
            engine,
            id,
            trace,
            params,
            json,
        } => {
            let skill = SkillRoot::load(skill_root)?;
            let atomic = skill
                .atomic(&id)
                .with_context(|| format!("未找到 atomic: {id}"))?;
            let parsed = parse_params(&params)?;
            let sql = prepare_sql(atomic, &parsed)?;
            let envelope = if engine == "mock" {
                let query_engine = MockTraceQueryEngine;
                query_engine.query(&atomic.id, &trace, &sql, &atomic.resources)?
            } else if engine == "perfetto" {
                let binary = std::env::var("HTRACE_TRACE_PROCESSOR")
                    .map(PathBuf::from)
                    .context("HTRACE_TRACE_PROCESSOR 必须指向 trace_processor binary")?;
                let query_engine = PerfettoShellEngine::new(binary);
                query_engine.query(&atomic.id, &trace, &sql, &atomic.resources)?
            } else {
                bail!("未知 engine: {engine}");
            };

            if json {
                println!("{}", serde_json::to_string(&envelope)?);
            } else {
                println!("{envelope:#?}");
            }
        }
    }
    Ok(())
}
