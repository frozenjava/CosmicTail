// SPDX-License-Identifier: MIT

//! Shared library behind both binaries.
//!
//! `cosmic_tail` is the main window; `cosmic_tail_applet` is the COSMIC panel
//! applet. A COSMIC panel applet has to be its own executable — the panel
//! launches it — so the parts they have in common ([`tailscale`] and [`ui`])
//! live here rather than in either one.

pub mod app;
pub mod applet;
pub mod config;
pub mod i18n;
pub mod panel;
pub mod tailscale;
pub mod ui;

/// Send `tracing` output to stderr.
///
/// Without this every `tracing::` call in the crate is discarded, which is
/// worth knowing when something goes wrong inside the panel: cosmic-panel
/// forwards an applet's stderr to the journal, prefixed with its app id, so
/// `journalctl --user -f` shows these live.
///
/// Defaults to warnings only. Anything that leaves the app silently not doing
/// what was asked is logged at that level for exactly this reason; set
/// `RUST_LOG=cosmic_tail=debug` for the rest.
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Confirms at a glance that logging is live and at what level — useful
    // when the process was started by the panel rather than from a shell.
    tracing::debug!(version = env!("CARGO_PKG_VERSION"), "logging initialised");
}
