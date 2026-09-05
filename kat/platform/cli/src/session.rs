use clap::{Args, Subcommand};
use serde::Serialize;

use crate::{
    locate_data_home,
    response::{self, PreparedResponse},
    session_store::SessionStore,
};

#[derive(Args)]
pub(super) struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Create one empty Analysis Session and publish its generated ID.
    Create,
    /// Permanently delete one Analysis Session and all of its contained state.
    Delete(DeleteSessionArgs),
}

#[derive(Args)]
struct DeleteSessionArgs {
    /// Select one exact published Analysis Session ID.
    #[arg(long, value_name = "SESSION_ID")]
    session: String,
}

#[derive(Serialize)]
pub(super) struct SessionResult {
    session_id: String,
}

pub(super) fn execute(arguments: SessionArgs) -> PreparedResponse<SessionResult> {
    match arguments.command {
        SessionCommand::Create => create(),
        SessionCommand::Delete(arguments) => delete(arguments),
    }
}

fn create() -> PreparedResponse<SessionResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    match SessionStore::new(&data_home).create() {
        Ok(opened) => {
            let result = SessionResult {
                session_id: opened.layout().session_id().as_str().to_owned(),
            };
            response::retain_session_lease(response::prepare_success(result), opened.into_lease())
        }
        Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
    }
}

fn delete(arguments: DeleteSessionArgs) -> PreparedResponse<SessionResult> {
    let data_home = match locate_data_home() {
        Ok(data_home) => data_home,
        Err(error) => return response::prepare_cli_failure(miette::Report::new(error)),
    };
    match SessionStore::new(&data_home).delete(&arguments.session) {
        Ok(session_id) => response::prepare_success(SessionResult {
            session_id: session_id.as_str().to_owned(),
        }),
        Err(error) => response::prepare_cli_failure(miette::Report::new(error)),
    }
}
