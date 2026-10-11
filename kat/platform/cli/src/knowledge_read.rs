use std::path::PathBuf;

use clap::{Args, Subcommand};
use miette::{IntoDiagnostic, Result};

use crate::{
    analysis_record::Material,
    analysis_store::{self, KnowledgeReceipt},
    knowledge::{Adoption, Selection},
    response::{self, PreparedResponse},
    session_store::SessionStore,
};

#[derive(Args)]
pub(super) struct KnowledgeArgs {
    #[command(subcommand)]
    command: KnowledgeCommand,
}

#[derive(Subcommand)]
enum KnowledgeCommand {
    /// Capture selected PACK knowledge or restore an existing snapshot.
    Read(ReadArgs),
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["entry", "path", "material"])))]
struct ReadArgs {
    #[arg(long)]
    session: String,
    #[arg(
        long,
        required_unless_present = "material",
        conflicts_with = "material"
    )]
    pack: Option<String>,
    #[arg(long)]
    entry: Option<String>,
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    material: Option<String>,
    #[arg(long, requires = "expected_revision", conflicts_with = "material")]
    update: bool,
    #[arg(long, requires = "update", conflicts_with = "material")]
    expected_revision: Option<u64>,
    #[arg(long = "pack-dir", conflicts_with = "material")]
    pack_directories: Vec<PathBuf>,
}

pub(super) fn execute(arguments: KnowledgeArgs) -> PreparedResponse<KnowledgeReceipt> {
    let KnowledgeCommand::Read(arguments) = arguments.command;
    let opened = match crate::locate_data_home()
        .into_diagnostic()
        .and_then(|home| {
            SessionStore::new(&home)
                .open(&arguments.session)
                .into_diagnostic()
        }) {
        Ok(opened) => opened,
        Err(error) => return response::prepare_cli_failure(error),
    };
    let result = (|| -> Result<KnowledgeReceipt> {
        if let Some(id) = &arguments.material {
            let record = analysis_store::read(opened.layout())?;
            let material = record.materials.get(id).ok_or_else(|| {
                miette::miette!("material is not present in this Analysis Record")
            })?;
            if !matches!(material, Material::Knowledge { .. }) {
                miette::bail!("material is not knowledge; use analysis show for query materials");
            }
            return Ok(KnowledgeReceipt::existing(&record, id, material));
        }
        let pack = arguments
            .pack
            .as_deref()
            .expect("clap requires a PACK for source selection");
        let selection = if let Some(name) = arguments.entry.as_deref() {
            Selection::Entry { pack, name }
        } else {
            Selection::Reference {
                pack,
                path: arguments
                    .path
                    .as_deref()
                    .expect("clap requires one knowledge target"),
            }
        };
        selection.validate()?;
        let request = Adoption {
            selection,
            update_revision: arguments.expected_revision,
        };
        analysis_store::adopt_knowledge(opened.layout(), &request, || {
            request.selection.load(arguments.pack_directories)
        })
    })();
    let prepared = match result {
        Ok(result) => response::prepare_success(result),
        Err(error) => response::prepare_cli_failure(error),
    };
    response::retain_session_lease(prepared, opened.into_lease())
}
