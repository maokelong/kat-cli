use crate::config::SkillRoot;
use crate::engine::mock::MockTraceQueryEngine;
use crate::engine::perfetto_shell::PerfettoShellEngine;
use crate::engine::TraceQueryEngine;
use crate::executor::params::prepare_sql;
use crate::replay::model::{ReplayPlan, ReplayRunSummary};
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct ReplayCommand {
    #[command(subcommand)]
    pub action: ReplayAction,
}

#[derive(Debug, Subcommand)]
pub enum ReplayAction {
    Run {
        replay: PathBuf,
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long)]
        trace: PathBuf,
        #[arg(long, default_value = "perfetto")]
        engine: String,
        #[arg(long)]
        json: bool,
    },
    Batch {
        replay: PathBuf,
        #[arg(long)]
        skill_root: PathBuf,
        #[arg(long = "trace")]
        traces: Vec<PathBuf>,
        #[arg(long, default_value = "1")]
        jobs: usize,
        #[arg(long, default_value = "perfetto")]
        engine: String,
    },
}

pub fn run(cmd: ReplayCommand) -> Result<()> {
    match cmd.action {
        ReplayAction::Run {
            replay,
            skill_root,
            trace,
            engine,
            json,
        } => {
            let summary = run_one(replay, skill_root, trace, engine)?;
            if json {
                println!("{}", serde_json::to_string(&summary)?);
            } else {
                println!("{summary:#?}");
            }
        }
        ReplayAction::Batch {
            replay,
            skill_root,
            traces,
            jobs,
            engine,
        } => {
            if jobs == 0 {
                bail!("jobs 必须大于 0");
            }
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(jobs)
                .build()
                .context("创建 replay worker pool")?;
            let summaries: Result<Vec<_>> = pool.install(|| {
                traces
                    .into_par_iter()
                    .map(|trace| run_one(replay.clone(), skill_root.clone(), trace, engine.clone()))
                    .collect()
            });
            for summary in summaries? {
                println!("{}", serde_json::to_string(&summary)?);
            }
        }
    }
    Ok(())
}

fn run_one(
    replay: PathBuf,
    skill_root: PathBuf,
    trace: PathBuf,
    engine: String,
) -> Result<ReplayRunSummary> {
    let plan_text =
        fs::read_to_string(&replay).with_context(|| format!("读取 {}", replay.display()))?;
    let plan: ReplayPlan = serde_norway::from_str(&plan_text).context("解析 replay yaml")?;
    let skill = SkillRoot::load(skill_root)?;
    let mut statuses = Vec::new();

    for step in &plan.steps {
        let atomic = skill
            .atomic(&step.atomic)
            .with_context(|| format!("未找到 atomic: {}", step.atomic))?;
        let sql = prepare_sql(atomic, &step.params)?;
        let status = if engine == "mock" {
            let query_engine = MockTraceQueryEngine;
            query_engine
                .query(&atomic.id, &trace, &sql, &atomic.resources)?
                .status
        } else if engine == "perfetto" {
            let binary = std::env::var("HTRACE_TRACE_PROCESSOR")
                .map(PathBuf::from)
                .context("HTRACE_TRACE_PROCESSOR 必须指向 trace_processor binary")?;
            let query_engine = PerfettoShellEngine::new(binary);
            query_engine
                .query(&atomic.id, &trace, &sql, &atomic.resources)?
                .status
        } else {
            bail!("未知 engine: {engine}");
        };
        statuses.push(status);
    }

    Ok(ReplayRunSummary {
        problem_signature: plan.problem_signature,
        source_strategy: plan.source_strategy,
        step_count: plan.steps.len(),
        statuses,
    })
}
