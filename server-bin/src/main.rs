// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless clipboardwire hub.
//!
//! This is the equivalent of `clipboardwire serve`, but as a standalone
//! binary that depends only on `clipboardwire-core`. It exists so the
//! Docker image can ship a tiny, GUI-free executable with no GTK/X11/OpenGL
//! runtime dependencies.
//!
//! All configuration comes from `CLIPBOARDWIRE_*` environment variables
//! (see `ARCHITECTURE.md` §2.4). There is no config file and no CLI flags:
//! a container is configured through its environment.

use anyhow::Result;
use clipboardwire_core::server::{self, ServerConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Honor RUST_LOG (default: info for our crates). Matches the cli binary's
    // logging shape closely enough for container stdout/stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "clipboardwire_core=info,clipboardwire_server=info".into()),
        )
        .init();

    let config = ServerConfig::from_env()?;
    tracing::info!(bind = %config.bind, "starting clipboardwire hub (headless)");
    server::run(config).await
}
