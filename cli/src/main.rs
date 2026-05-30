use anyhow::Result;
use clap::{Parser, Subcommand};
use htrace::commands::{atomic, profile, replay, run, strategy};

#[derive(Debug, Parser)]
#[command(name = "htrace")]
#[command(about = "面向 OpenCode skill 的鸿蒙 trace 分析运行时")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Profile(profile::ProfileCommand),
    Strategy(strategy::StrategyCommand),
    Atomic(atomic::AtomicCommand),
    Replay(replay::ReplayCommand),
    Run(run::RunCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
        Command::Profile(cmd) => profile::run(cmd)?,
        Command::Strategy(cmd) => strategy::run(cmd)?,
        Command::Atomic(cmd) => atomic::run(cmd)?,
        Command::Replay(cmd) => replay::run(cmd)?,
        Command::Run(cmd) => run::run(cmd)?,
    }
    Ok(())
}
