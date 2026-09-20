use std::{fs::File, path::PathBuf};

use clap::{Args, Subcommand};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    analysis_record::{AnalysisRecord, Change, Material},
    analysis_store,
    response::{self, PreparedResponse},
    session_store::SessionStore,
};

#[derive(Args)]
pub(super) struct AnalysisArgs {
    #[command(subcommand)]
    command: AnalysisCommand,
}

#[derive(Subcommand)]
enum AnalysisCommand {
    /// Initialize the existing Session's analysis goal exactly once.
    Init {
        #[arg(long)]
        session: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Restore the saved report, one node, or one original material.
    Show {
        #[arg(long)]
        session: String,
        #[arg(long, conflicts_with = "material")]
        run: Option<String>,
        #[arg(long, conflicts_with = "run")]
        material: Option<String>,
    },
    /// Apply a complete batch of domain changes if the revision still matches.
    Update {
        #[arg(long)]
        session: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Changes {
    changes: Vec<Change>,
}

pub(super) fn execute(arguments: AnalysisArgs) -> PreparedResponse<Value> {
    let session_id = match &arguments.command {
        AnalysisCommand::Init { session, .. }
        | AnalysisCommand::Show { session, .. }
        | AnalysisCommand::Update { session, .. } => session,
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
            AnalysisCommand::Init { file, .. } => {
                let record = analysis_store::init(opened.layout(), input(&file)?)?;
                Ok(overview(&record))
            }
            AnalysisCommand::Update {
                expected_revision,
                file,
                ..
            } => {
                let changes: Changes = input(&file)?;
                let record =
                    analysis_store::update(opened.layout(), expected_revision, changes.changes)?;
                Ok(overview(&record))
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
                    Ok(overview(&record))
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

fn overview(record: &AnalysisRecord) -> Value {
    let materials: serde_json::Map<String, Value> = record.materials.iter().map(|(id, material)| {
        let summary = match material {
            Material::Guide {pack,workflow,guide} => json!({"kind":"guide","pack":pack,"workflow":workflow,"has_guide":guide.is_some()}),
            Material::Query {run_id,..} => json!({"kind":"query","run_id":run_id}),
        };
        (id.clone(),summary)
    }).collect();
    json!({
        "schema_version":record.schema_version,
        "session_id":record.session_id,
        "revision":record.revision,
        "goal":record.goal,
        "nodes":record.nodes,
        "materials":materials,
        "report":record.report,
    })
}
