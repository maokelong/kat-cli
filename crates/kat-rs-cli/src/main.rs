use clap::Parser;
use kat_rs_cli::{commands, logging};

#[tokio::main]
async fn main() {
    logging::init();

    let cli = commands::Cli::parse();
    let code = commands::run(cli, &mut std::io::stdout(), &mut std::io::stderr()).await;

    std::process::exit(code);
}
