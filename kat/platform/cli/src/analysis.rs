use std::{fs::File, path::PathBuf};

use clap::{Args, Subcommand};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde_json::{Value, json};

use crate::{
    analysis_record::{AnalysisRecord, Material, ReportNode},
    analysis_store,
    response::{self, PreparedResponse},
    run_manifest,
    session_store::SessionStore,
};

#[derive(Args)]
pub(super) struct AnalysisArgs {
    #[command(subcommand)]
    command: AnalysisCommand,
}

#[derive(Subcommand)]
enum AnalysisCommand {
    /// Restore the saved report, one node, or one original material.
    Show {
        #[arg(long)]
        session: String,
        #[arg(long, conflicts_with = "material")]
        run: Option<String>,
        #[arg(long, conflicts_with = "run")]
        material: Option<String>,
    },
    /// Save incremental content; revision 0 creates a record with its first goal.
    Save {
        #[arg(long)]
        session: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        file: PathBuf,
    },
}

pub(super) fn execute(arguments: AnalysisArgs) -> PreparedResponse<Value> {
    let session_id = match &arguments.command {
        AnalysisCommand::Show { session, .. } | AnalysisCommand::Save { session, .. } => session,
    };
    let opened = match crate::locate_data_home()
        .into_diagnostic()
        .and_then(|home| SessionStore::new(&home).open(session_id).into_diagnostic())
    {
        Ok(opened) => opened,
        Err(error) => return response::prepare_cli_failure(error),
    };
    let result = (|| -> Result<Value> {
        match arguments.command {
            AnalysisCommand::Save {
                expected_revision,
                file,
                ..
            } => {
                let (record, nodes) =
                    analysis_store::save(opened.layout(), expected_revision, input(&file)?)?;
                Ok(overview(&record, nodes))
            }
            AnalysisCommand::Show { run, material, .. } => {
                let record = analysis_store::read(opened.layout())?;
                if let Some(run_id) = run {
                    let node = record
                        .nodes
                        .iter()
                        .find(|node| node.run_id == run_id)
                        .ok_or_else(|| {
                            miette::miette!("Run is not selected in this Analysis Record")
                        })?;
                    Ok(
                        json!({"session_id":record.session_id,"revision":record.revision,"node":node}),
                    )
                } else if let Some(id) = material {
                    let material = record.materials.get(&id).ok_or_else(|| {
                        miette::miette!("material is not present in this Analysis Record")
                    })?;
                    Ok(
                        json!({"session_id":record.session_id,"revision":record.revision,"material_id":id,"material":material}),
                    )
                } else {
                    let runs = run_manifest::resolve_all(opened.layout()).into_diagnostic()?;
                    let nodes = record.report_tree(&runs)?;
                    Ok(overview(&record, nodes))
                }
            }
        }
    })();
    let prepared = match result {
        Ok(result) => response::prepare_success(result),
        Err(error) => response::prepare_cli_failure(error),
    };
    response::retain_session_lease(prepared, opened.into_lease())
}

fn input<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T> {
    let file = File::open(path)
        .into_diagnostic()
        .wrap_err("cannot open analysis input file")?;
    serde_json::from_reader(file)
        .into_diagnostic()
        .wrap_err("invalid analysis input")
}

fn overview(record: &AnalysisRecord, nodes: Vec<ReportNode>) -> Value {
    let materials: serde_json::Map<String, Value> = record
        .materials
        .iter()
        .map(|(id, material)| {
            let summary = match material {
                Material::Query { run_id, .. } => json!({"kind":"query","run_id":run_id}),
            };
            (id.clone(), summary)
        })
        .collect();
    json!({
        "schema_version":record.schema_version,
        "session_id":record.session_id,
        "revision":record.revision,
        "goal":record.goal,
        "nodes":nodes,
        "materials":materials,
        "report":record.report,
    })
}
