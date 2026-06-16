use std::net::SocketAddr;

use crate::{AppState, DaemonConfig, router};

pub async fn serve(config: DaemonConfig) -> anyhow::Result<SocketAddr> {
    if !config.host.is_loopback() {
        anyhow::bail!("daemon host must be a loopback IP address");
    }

    let listener = tokio::net::TcpListener::bind(config.socket_addr()).await?;
    let local_addr = listener.local_addr()?;
    let state = AppState::new(1);
    let shutdown = state.shutdown.clone();

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = shutdown.notified() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
        })
        .await?;

    Ok(local_addr)
}
