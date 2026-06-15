use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    time::Duration,
};

use anyhow::anyhow;
use clap::{Args, Parser, Subcommand, ValueEnum};

const DAEMON_STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Parser)]
#[command(name = "kat-rs")]
#[command(about = "Query trace and log files with SQL")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Daemon(DaemonArgs),
    Query(QueryArgs),
}

#[derive(Clone, Debug, Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub command: DaemonCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DaemonCommand {
    Start(DaemonEndpointArgs),
    Stop(DaemonEndpointArgs),
}

#[derive(Clone, Copy, Debug, Args)]
pub struct DaemonEndpointArgs {
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub host: IpAddr,
    #[arg(long, default_value_t = 3030)]
    pub port: u16,
}

#[derive(Clone, Debug, Args)]
pub struct QueryArgs {
    #[arg(long, value_enum)]
    pub source: SourceArg,
    #[arg(
        long,
        required_if_eq("source", "hitrace"),
        conflicts_with_all = ["observations_file", "traces_file"]
    )]
    pub file: Option<PathBuf>,
    #[arg(long, required_if_eq("source", "langfuse"), conflicts_with = "file")]
    pub observations_file: Option<PathBuf>,
    #[arg(long, required_if_eq("source", "langfuse"), conflicts_with = "file")]
    pub traces_file: Option<PathBuf>,
    #[arg(long)]
    pub sql: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SourceArg {
    Hitrace,
    Langfuse,
}

impl SourceArg {
    fn as_str(self) -> &'static str {
        match self {
            SourceArg::Hitrace => "hitrace",
            SourceArg::Langfuse => "langfuse",
        }
    }
}

pub async fn run(cli: Cli, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match run_inner(cli, out).await {
        Ok(()) => 0,
        Err(CommandError::Runtime(error)) => {
            log::error!("command failed: {error:#}");
            let _ = writeln!(err, "{error:#}");
            1
        }
    }
}

async fn run_inner(cli: Cli, out: &mut dyn Write) -> Result<(), CommandError> {
    match cli.command {
        Command::Daemon(args) => run_daemon(args).await,
        Command::Query(args) => run_query(args, out).await,
    }
}

async fn run_daemon(args: DaemonArgs) -> Result<(), CommandError> {
    match args.command {
        DaemonCommand::Start(args) => run_daemon_start(args).await,
        DaemonCommand::Stop(args) => run_daemon_stop(args),
    }
}

async fn run_daemon_start(args: DaemonEndpointArgs) -> Result<(), CommandError> {
    ensure_loopback_host(args.host)?;

    kat_rs_daemon::serve(kat_rs_daemon::DaemonConfig {
        host: args.host,
        port: args.port,
    })
    .await
    .map(|_| ())
    .map_err(CommandError::from_runtime)
}

fn run_daemon_stop(args: DaemonEndpointArgs) -> Result<(), CommandError> {
    ensure_loopback_host(args.host)?;

    let addr = SocketAddr::new(args.host, args.port);
    let mut stream = TcpStream::connect_timeout(&addr, DAEMON_STOP_TIMEOUT).map_err(|error| {
        CommandError::from_runtime(anyhow!("daemon stop failed to connect to {addr}: {error}"))
    })?;
    stream
        .set_read_timeout(Some(DAEMON_STOP_TIMEOUT))
        .map_err(|error| {
            CommandError::from_runtime(anyhow!(
                "daemon stop failed to set read timeout for {addr}: {error}"
            ))
        })?;
    stream
        .set_write_timeout(Some(DAEMON_STOP_TIMEOUT))
        .map_err(|error| {
            CommandError::from_runtime(anyhow!(
                "daemon stop failed to set write timeout for {addr}: {error}"
            ))
        })?;

    let request = format!(
        "DELETE /v1/server HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        args.host, args.port
    );

    stream.write_all(request.as_bytes()).map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "daemon stop failed to send shutdown request to {addr}: {error}"
        ))
    })?;
    stream.flush().map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "daemon stop failed to flush shutdown request to {addr}: {error}"
        ))
    })?;

    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "daemon stop failed to read shutdown response from {addr}: {error}"
        ))
    })?;

    let status_line = response.lines().next().unwrap_or("<empty response>");
    let status = status_line.split_whitespace().nth(1);
    if status == Some("202") {
        Ok(())
    } else {
        Err(CommandError::from_runtime(anyhow!(
            "daemon stop failed: expected HTTP 202, got {status_line}"
        )))
    }
}

fn ensure_loopback_host(host: IpAddr) -> Result<(), CommandError> {
    if host.is_loopback() {
        Ok(())
    } else {
        Err(CommandError::from_runtime(anyhow!(
            "daemon host must be a loopback IP address"
        )))
    }
}

async fn run_query(args: QueryArgs, out: &mut dyn Write) -> Result<(), CommandError> {
    let datasource = match args.source {
        SourceArg::Hitrace => {
            let file = required_path_arg(args.file, "--file", args.source)?;
            kat_rs_datasource::TraceDatasource::from_hitrace(file)
        }
        SourceArg::Langfuse => {
            let observations_file =
                required_path_arg(args.observations_file, "--observations-file", args.source)?;
            let traces_file = required_path_arg(args.traces_file, "--traces-file", args.source)?;
            kat_rs_datasource::TraceDatasource::from_langfuse_legacy(observations_file, traces_file)
                .await
        }
    }
    .map_err(CommandError::from_runtime)?;

    let rows = datasource
        .query_json(&args.sql)
        .await
        .map_err(CommandError::from_runtime)?;

    serde_json::to_writer(&mut *out, &rows).map_err(CommandError::from_runtime)?;
    writeln!(out).map_err(CommandError::from_runtime)?;
    Ok(())
}

fn required_path_arg(
    value: Option<PathBuf>,
    name: &'static str,
    source: SourceArg,
) -> Result<PathBuf, CommandError> {
    value.ok_or_else(|| {
        CommandError::from_runtime(anyhow!(
            "{name} is required when --source {}",
            source.as_str()
        ))
    })
}

enum CommandError {
    Runtime(anyhow::Error),
}

impl CommandError {
    fn from_runtime(error: impl Into<anyhow::Error>) -> Self {
        Self::Runtime(error.into())
    }
}
