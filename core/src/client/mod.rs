// SPDX-License-Identifier: GPL-3.0-or-later

//! Clipboard client: arboard-driven poll loop, WebSocket transport,
//! supervisor that ties them together with echo-loop suppression.

pub mod clipboard;
pub mod config;
pub mod transport;

use anyhow::Result;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::protocol::{ClipFrame, SUPPORTED_CONTENT_TYPE};

pub use clipboard::Clipboard;
pub use config::ClientConfig;
pub use transport::Transport;

/// Run the clipboard client until the process is signalled or an unrecoverable
/// error occurs. Spawns the arboard thread, the transport task, and bridges
/// them in a single select loop.
pub async fn run(config: ClientConfig) -> Result<()> {
    let poll_ms = config.poll_ms;
    let (clipboard, _clipboard_join) = clipboard::spawn(poll_ms)?;
    let (transport, _transport_join) = transport::spawn(config);
    run_supervisor(clipboard, transport).await;
    Ok(())
}

/// Bridge clipboard events ↔ transport frames. Public for the integration
/// tests, which inject test-controlled [`Clipboard`] and [`Transport`] handles
/// without spawning the real arboard thread or hitting the network.
pub async fn run_supervisor(mut clipboard: Clipboard, mut transport: Transport) {
    loop {
        tokio::select! {
            local = clipboard.events_rx.recv() => {
                let Some(text) = local else {
                    debug!("clipboard events channel closed; supervisor exiting");
                    return;
                };
                let frame = ClipFrame {
                    id: Uuid::new_v4(),
                    ts: now_millis(),
                    content_type: SUPPORTED_CONTENT_TYPE.to_string(),
                    content: text,
                    from: None,
                };
                if let Err(e) = transport.outbound_tx.try_send(frame) {
                    warn!(error = %e, "transport outbound full or closed; dropping local change");
                }
            }
            remote = transport.inbound_rx.recv() => {
                let Some(clip) = remote else {
                    debug!("transport inbound channel closed; supervisor exiting");
                    return;
                };
                if clip.content_type != SUPPORTED_CONTENT_TYPE {
                    warn!(content_type = %clip.content_type, "ignoring inbound clip with unsupported content_type");
                    continue;
                }
                if let Err(e) = clipboard.apply_tx.send(clip.content) {
                    warn!(error = %e, "clipboard thread is gone");
                    return;
                }
            }
            else => return,
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
