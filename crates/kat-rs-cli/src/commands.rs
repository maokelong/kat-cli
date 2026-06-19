use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::Duration,
};

use anyhow::anyhow;
use clap::{Args, Parser, Subcommand};

const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Parser)]
#[command(name = "kat-rs")]
#[command(about = "Run the local kat-rs API server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Serve(ServerEndpointArgs),
    Stop(ServerEndpointArgs),
    Openapi,
    Version,
}

#[derive(Clone, Copy, Debug, Args)]
pub struct ServerEndpointArgs {
    #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
    pub host: IpAddr,
    #[arg(long, default_value_t = 3030)]
    pub port: u16,
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
        Command::Serve(args) => run_serve(args).await,
        Command::Stop(args) => run_stop(args),
        Command::Openapi => run_openapi(out),
        Command::Version => run_version(out),
    }
}

async fn run_serve(args: ServerEndpointArgs) -> Result<(), CommandError> {
    ensure_loopback_host(args.host)?;

    kat_rs_daemon::serve(kat_rs_daemon::DaemonConfig {
        host: args.host,
        port: args.port,
    })
    .await
    .map(|_| ())
    .map_err(CommandError::from_runtime)
}

fn run_stop(args: ServerEndpointArgs) -> Result<(), CommandError> {
    ensure_loopback_host(args.host)?;

    let addr = SocketAddr::new(args.host, args.port);
    let mut stream = TcpStream::connect_timeout(&addr, SERVER_STOP_TIMEOUT).map_err(|error| {
        CommandError::from_runtime(anyhow!("server stop failed to connect to {addr}: {error}"))
    })?;
    stream
        .set_read_timeout(Some(SERVER_STOP_TIMEOUT))
        .map_err(|error| {
            CommandError::from_runtime(anyhow!(
                "server stop failed to set read timeout for {addr}: {error}"
            ))
        })?;
    stream
        .set_write_timeout(Some(SERVER_STOP_TIMEOUT))
        .map_err(|error| {
            CommandError::from_runtime(anyhow!(
                "server stop failed to set write timeout for {addr}: {error}"
            ))
        })?;

    let request = format!(
        "DELETE /v1/server HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        args.host, args.port
    );

    stream.write_all(request.as_bytes()).map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "server stop failed to send shutdown request to {addr}: {error}"
        ))
    })?;
    stream.flush().map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "server stop failed to flush shutdown request to {addr}: {error}"
        ))
    })?;

    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|error| {
        CommandError::from_runtime(anyhow!(
            "server stop failed to read shutdown response from {addr}: {error}"
        ))
    })?;

    let status_line = response.lines().next().unwrap_or("<empty response>");
    let status = status_line.split_whitespace().nth(1);
    if status == Some("202") {
        Ok(())
    } else {
        Err(CommandError::from_runtime(anyhow!(
            "server stop failed: expected HTTP 202, got {status_line}"
        )))
    }
}

fn run_openapi(out: &mut dyn Write) -> Result<(), CommandError> {
    serde_json::to_writer_pretty(&mut *out, &kat_rs_daemon::openapi_document())
        .map_err(CommandError::from_runtime)?;
    writeln!(out).map_err(CommandError::from_runtime)?;
    Ok(())
}

fn run_version(out: &mut dyn Write) -> Result<(), CommandError> {
    writeln!(out, "{}", env!("CARGO_PKG_VERSION")).map_err(CommandError::from_runtime)
}

fn ensure_loopback_host(host: IpAddr) -> Result<(), CommandError> {
    if host.is_loopback() {
        Ok(())
    } else {
        Err(CommandError::from_runtime(anyhow!(
            "server host must be a loopback IP address"
        )))
    }
}

enum CommandError {
    Runtime(anyhow::Error),
}

impl CommandError {
    fn from_runtime(error: impl Into<anyhow::Error>) -> Self {
        Self::Runtime(error.into())
    }
}
