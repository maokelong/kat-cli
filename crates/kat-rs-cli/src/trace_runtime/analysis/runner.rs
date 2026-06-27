use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::sqlite::SQLiteDatasetAdapter,
    analysis::{
        context::AnalysisState,
        derived::DerivedRunner,
        run_store::AnalysisRunStore,
        steps::{
            evidence::render_seed_evidence, graph_walk::run_graph_walk_on_rows,
            report::run_report_render,
        },
    },
    pack::{LoadedPack, spec::AnalysisStepSpec},
};

pub struct AnalysisRunConfig {
    pub raw_db: PathBuf,
    pub scratch_db: PathBuf,
    pub run_root: PathBuf,
    pub run_id: String,
    pub pack: LoadedPack,
    pub analysis_id: String,
    pub params: Value,
}

pub fn run_analysis(config: AnalysisRunConfig) -> Result<PathBuf> {
    let mut adapter = SQLiteDatasetAdapter::open(&config.raw_db, &config.scratch_db)
        .with_context(|| format!("failed to open raw db {}", config.raw_db.display()))?;
    let analysis = config
        .pack
        .analyses
        .iter()
        .find(|analysis| analysis.id == config.analysis_id)
        .with_context(|| format!("analysis `{}` not found in pack", config.analysis_id))?;
    let store = AnalysisRunStore::create(&config.run_root, &config.run_id)?;
    let mut derived = DerivedRunner::new(&config.pack)?;
    let mut state = AnalysisState::default();
    let mut evidence = Vec::new();

    store.write_plan(&serde_json::to_value(analysis)?)?;

    for step in &analysis.steps {
        match step {
            AnalysisStepSpec::EvidenceRender(step) => {
                derived.ensure_table(&mut adapter, &step.from, &config.params, state.value())?;
                let rows = adapter.query_json(&format!("SELECT * FROM {}", step.from))?;
                let item = render_seed_evidence(&step.id, &step.from, &rows, &mut state)?;
                store.append_evidence(&item)?;
                evidence.push(item);
            }
            AnalysisStepSpec::TemporalGraphWalk(step) => {
                let mut table_rows = Vec::new();
                let mut tables = BTreeSet::new();
                for provider in &step.edge_providers {
                    tables.insert(provider.table.clone());
                    for table in &provider.emit.evidence {
                        tables.insert(table.clone());
                    }
                    for fact in provider.emit.facts.values() {
                        if let Some(table) = &fact.table {
                            tables.insert(table.clone());
                        }
                    }
                }

                for table in &tables {
                    derived.ensure_table(&mut adapter, table, &config.params, state.value())?;
                    let rows = adapter.query_json(&format!("SELECT * FROM {table}"))?;
                    table_rows.push((table.as_str(), rows));
                }

                for item in run_graph_walk_on_rows(step, &mut state, &table_rows)? {
                    store.append_evidence(&item)?;
                    evidence.push(item);
                }
            }
            AnalysisStepSpec::GraphWalk(step) => {
                bail!("graph.walk step `{}` is not executable yet", step.id);
            }
            AnalysisStepSpec::ReportRender(_) => {
                let report = run_report_render(state.value(), &evidence)?;
                store.write_report(&report)?;
            }
        }
    }

    store.write_state(state.value())?;
    store.render_checklist()?;
    Ok(config.run_root.join(config.run_id))
}
