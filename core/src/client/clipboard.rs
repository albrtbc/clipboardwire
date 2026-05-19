// SPDX-License-Identifier: GPL-3.0-or-later

//! Dedicated-thread clipboard adapter.
//!
//! The `arboard::Clipboard` handle is created and owned by a single OS thread
//! (X11 / Wayland / Win32 / NSPasteboard all benefit from a stable owner).
//! The thread runs a single hybrid loop that does two things on each tick:
//!
//! 1. **Apply** any pending write requests (from the supervisor).
//! 2. **Poll** the local clipboard for changes and emit any new value.
//!
//! Both directions are exposed as channels so the supervisor is portable and
//! easily mockable: tests construct a [`Clipboard`] directly with their own
//! channel ends, without ever spawning the real arboard thread.
//!
//! Echo-loop suppression: when we apply an inbound clip, we update the
//! thread's `last_seen` tracker to the value we just wrote. The next poll
//! won't see it as "new" and so won't bounce it back to the hub.

use std::sync::mpsc as smpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};

/// Default-buffer size for the outbound events channel.
const EVENT_BUFFER: usize = 32;

/// Async-side handle to a clipboard:
/// - `events_rx` yields the local clipboard's content whenever it changes.
/// - `apply_tx` requests that a value be written to the local clipboard.
///
/// Construct via [`spawn`] for the production arboard-backed loop, or
/// directly (the fields are `pub`) for tests.
pub struct Clipboard {
    pub events_rx: mpsc::Receiver<String>,
    pub apply_tx: smpsc::Sender<String>,
}

/// Spawn the arboard-backed clipboard thread. Returns the [`Clipboard`] handle
/// and a [`JoinHandle`] so callers can wait on graceful shutdown.
pub fn spawn(poll_ms: u64) -> Result<(Clipboard, JoinHandle<()>)> {
    let (apply_tx, apply_rx) = smpsc::channel::<String>();
    let (events_tx, events_rx) = mpsc::channel::<String>(EVENT_BUFFER);
    let join = std::thread::Builder::new()
        .name("clipboardwire-clipboard".into())
        .spawn(move || run_arboard_loop(apply_rx, events_tx, poll_ms))?;
    Ok((
        Clipboard {
            events_rx,
            apply_tx,
        },
        join,
    ))
}

fn run_arboard_loop(
    apply_rx: smpsc::Receiver<String>,
    events_tx: mpsc::Sender<String>,
    poll_ms: u64,
) {
    let mut cb = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "arboard initialization failed; clipboard thread exiting");
            return;
        }
    };
    debug!(poll_ms, "clipboard thread started");

    let mut last_seen: Option<String> = None;
    let poll = Duration::from_millis(poll_ms);
    let mut next_poll = Instant::now() + poll;

    loop {
        let now = Instant::now();
        let wait = next_poll.saturating_duration_since(now);

        // Wait for the next write request, but no longer than until the next
        // scheduled poll.
        match apply_rx.recv_timeout(wait) {
            Ok(text) => {
                trace!(len = text.len(), "applying clip");
                if let Err(e) = cb.set_text(text.clone()) {
                    warn!(error = %e, "clipboard set_text failed");
                } else {
                    // Suppress the echo we'd otherwise read back from the next poll.
                    last_seen = Some(text);
                }
                // Keep draining writes back-to-back if more are queued —
                // applying everything before we poll keeps us responsive.
                continue;
            }
            Err(smpsc::RecvTimeoutError::Timeout) => {}
            Err(smpsc::RecvTimeoutError::Disconnected) => {
                debug!("apply channel closed; clipboard thread exiting");
                return;
            }
        }

        next_poll = Instant::now() + poll;

        match cb.get_text() {
            Ok(text) => {
                if last_seen.as_deref() != Some(text.as_str()) {
                    last_seen = Some(text.clone());
                    match events_tx.try_send(text) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            warn!("events channel full; dropping clipboard change");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            debug!("events channel closed; clipboard thread exiting");
                            return;
                        }
                    }
                }
            }
            Err(arboard::Error::ContentNotAvailable) => {
                // Empty or non-text clipboard. Forget any cached text so a
                // future text value will be reported as a change.
                last_seen = None;
            }
            Err(e) => {
                // Most often a transient X11/Wayland error while another app
                // owns the clipboard. Don't spam logs.
                trace!(error = %e, "clipboard get_text transient failure");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The arboard backend can't be exercised without a display; these tests
    // construct Clipboard directly with test-controlled channels.

    #[tokio::test]
    async fn supervisor_receives_events_via_channel() {
        let (apply_tx, _apply_rx) = smpsc::channel::<String>();
        let (events_tx, events_rx) = mpsc::channel::<String>(4);
        let cb = Clipboard {
            events_rx,
            apply_tx,
        };

        // Simulate the clipboard thread emitting an event.
        events_tx.send("hello".to_string()).await.unwrap();
        drop(events_tx);

        let mut cb = cb;
        let v = cb.events_rx.recv().await.unwrap();
        assert_eq!(v, "hello");
    }

    #[test]
    fn supervisor_can_send_apply_requests() {
        let (apply_tx, apply_rx) = smpsc::channel::<String>();
        let (_events_tx, events_rx) = mpsc::channel::<String>(4);
        let cb = Clipboard {
            events_rx,
            apply_tx,
        };

        cb.apply_tx.send("write me".into()).unwrap();
        let got = apply_rx.recv().unwrap();
        assert_eq!(got, "write me");
    }
}
