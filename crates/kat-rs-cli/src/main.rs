mod commands;
mod logging;

use clap::Parser;

#[tokio::main]
async fn main() {
    logging::init();

    let cli = commands::Cli::parse();
    let code = commands::run(cli, &mut std::io::stdout(), &mut std::io::stderr()).await;

    std::process::exit(code);
}
