use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::trace_runtime::{
    adapter::sqlite::SQLiteDatasetAdapter,
    analysis::{
        graph::walk::run_graph_walk_on_rows_v2, report::render_report, run_store::AnalysisRunStore,
        sqlite_rows::select_table_rows, state::AnalysisState,
        steps::seed_evidence::render_seed_evidence,
    },
    pack::{LoadedPack, spec::AnalysisStepSpec},
    transform::derived_runner::DerivedRunner,
};

const ANALYSIS_TABLE_ROW_LIMIT: usize = 10_000;

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
                let rows = select_table_rows(&mut adapter, &step.from, ANALYSIS_TABLE_ROW_LIMIT)?;
                let item = render_seed_evidence(&step.id, &step.from, &rows, &mut state)?;
                store.append_evidence(&item)?;
                evidence.push(item);
            }
            AnalysisStepSpec::GraphWalk(step) => {
                let mut table_rows = Vec::new();
                let mut tables = BTreeSet::new();
                for provider in &step.providers {
                    tables.insert(provider.input.table.clone());
                    for table in &provider.output.evidence.tables {
                        tables.insert(table.clone());
                    }
                }

                for table in &tables {
                    derived.ensure_table(&mut adapter, table, &config.params, state.value())?;
                    let rows = select_table_rows(&mut adapter, table, ANALYSIS_TABLE_ROW_LIMIT)?;
                    table_rows.push((table.as_str(), rows));
                }

                for item in
                    run_graph_walk_on_rows_v2(step, &mut state, &config.params, &table_rows)?
                {
                    store.append_evidence(&item)?;
                    evidence.push(item);
                }
            }
            AnalysisStepSpec::ReportRender(_) => {
                let report = render_report(state.value(), &evidence)?;
                store.write_report(&report)?;
            }
        }
    }

    store.write_state(state.value())?;
    store.render_checklist()?;
    Ok(config.run_root.join(config.run_id))
}
