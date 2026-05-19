// SPDX-License-Identifier: GPL-3.0-or-later

//! Hub server: WebSocket relay, in-memory last-clip cache, Basic auth.

pub mod auth;
pub mod config;
pub mod hub;
pub mod ws;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::routing::get;
use axum::Router;
use tokio::sync::Semaphore;
use tracing::info;

pub use config::ServerConfig;
pub use ws::AppState;

/// Build the axum router from a pre-constructed [`AppState`]. Exposed for tests.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/sync", get(ws::sync_handler))
        .route("/healthz", get(healthz))
        .with_state(state)
}

/// Spin up the hub task and build a fully-stateful axum router. Returns the
/// router plus the hub's join handle (useful for orderly shutdown in tests).
pub fn build_app(config: ServerConfig) -> (Router, tokio::task::JoinHandle<()>) {
    let (hub, hub_join) = hub::spawn(config.max_conns);
    let conn_sem = Arc::new(Semaphore::new(config.max_conns));
    let state = AppState {
        hub,
        config: Arc::new(config),
        conn_sem,
    };
    (router(state), hub_join)
}

async fn healthz() -> &'static str {
    "ok\n"
}

/// Bind a TcpListener without starting the server. Returns the listener and
/// the actual local address (which may differ from `config.bind` when port 0
/// is requested).
pub async fn bind(
    config: &ServerConfig,
) -> Result<(tokio::net::TcpListener, std::net::SocketAddr)> {
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    let addr = listener.local_addr().unwrap_or(config.bind);
    Ok((listener, addr))
}

/// Serve the hub on a pre-bound listener until `shutdown` resolves. Speaks
/// `wss://` if `config.tls_enabled()`, plain `ws://` otherwise.
pub async fn serve(
    listener: tokio::net::TcpListener,
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    if config.tls_enabled() {
        serve_tls(listener, config, shutdown).await
    } else {
        let (app, _hub_join) = build_app(config);
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .context("axum::serve")?;
        Ok(())
    }
}

async fn serve_tls(
    listener: tokio::net::TcpListener,
    config: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let cert = config
        .tls_cert_file
        .as_ref()
        .expect("tls_enabled() returned true with no cert file");
    let key = config
        .tls_key_file
        .as_ref()
        .expect("tls_enabled() returned true with no key file");
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
        .await
        .with_context(|| {
            format!(
                "loading TLS cert/key from {} and {}",
                cert.display(),
                key.display()
            )
        })?;

    let std_listener = listener
        .into_std()
        .context("converting tokio listener to std")?;
    std_listener
        .set_nonblocking(true)
        .context("set_nonblocking on TLS listener")?;
    let handle = axum_server::Handle::new();
    let handle_for_shutdown = handle.clone();
    tokio::spawn(async move {
        shutdown.await;
        handle_for_shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
    });

    let (app, _hub_join) = build_app(config);
    axum_server::from_tcp_rustls(std_listener, tls)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .context("axum_server::serve (tls)")?;
    Ok(())
}

/// Bind + serve in one call, using Ctrl-C / SIGTERM as the shutdown trigger.
pub async fn run(config: ServerConfig) -> Result<()> {
    let (listener, addr) = bind(&config).await?;
    info!(addr = %addr, "listening");
    serve(listener, config, shutdown_signal()).await?;
    info!("server exited");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => return std::future::pending::<()>().await,
        };
        sig.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown signal received");
}
