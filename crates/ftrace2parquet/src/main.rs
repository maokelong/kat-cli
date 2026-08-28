use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(about = "Convert a UTF-8 text ftrace file to one Parquet file")]
struct Cli {
    #[arg(long, value_name = "TRACE")]
    input: PathBuf,
    #[arg(long, value_name = "PARQUET")]
    output: PathBuf,
    #[arg(long, value_name = "NAME")]
    clock_domain: String,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = ftrace2parquet::convert(&cli.input, &cli.output, &cli.clock_domain) {
        eprintln!("ftrace2parquet failed: {error:#}");
        std::process::exit(1);
    }
}
