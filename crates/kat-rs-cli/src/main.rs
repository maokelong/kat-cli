mod commands;
mod logging;

#[tokio::main]
async fn main() {
    logging::init();

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let code = commands::run(&args, &mut std::io::stdout(), &mut std::io::stderr()).await;

    std::process::exit(code);
}
