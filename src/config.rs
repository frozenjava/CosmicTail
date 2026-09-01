// SPDX-License-Identifier: MIT

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

/// Persisted between runs.
///
/// This exists mostly for the applet's relaunch: Wayland offers no way to
/// restore a minimized window, so "Open Cosmic Tail" replaces it with a fresh
/// one, and without this the replacement would forget where you were.
///
/// The search text is deliberately not kept. A window that reopens with a
/// filter already applied looks like a window that has lost most of your
/// devices.
#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    /// Stable id of the device whose details were open in the device list.
    pub selected_device: Option<String>,
    /// The same, for the exit-node list. Kept apart because the two lists
    /// remember their own selection.
    pub selected_exit_node: Option<String>,
    /// True if the exit-node list was showing rather than the device list.
    pub exit_nodes_page: bool,
}
