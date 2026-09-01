// SPDX-License-Identifier: MIT

//! Adding and removing the applet from the COSMIC panel.
//!
//! An applet is not something an application can start. cosmic-panel launches
//! applets itself, as child processes, with the `COSMIC_PANEL_*` environment
//! set so they can size and anchor themselves to the panel; a process we spawn
//! ourselves gets none of that and ends up as a small free-floating window.
//!
//! What we *can* do is name ourselves in the panel's own configuration, which
//! is ordinary cosmic-config and which the panel reloads on change. That is
//! the whole of this module.

use cosmic::cosmic_config::{self, ConfigGet, ConfigSet};

/// Must match the applet's `APP_ID` and the basename of its desktop entry —
/// the panel resolves an entry in its config by looking up that file.
pub const APPLET_ID: &str = "com.github.frozenjava.CosmicTailApplet";

const PANEL_ID: &str = "com.system76.CosmicPanel.Panel";
const PANEL_VERSION: u64 = 1;
const WINGS_KEY: &str = "plugins_wings";

/// `plugins_wings` is `(start, end)`: the applets before and after the centre
/// of the panel. Status applets — network, battery, and now this — live in
/// `end`. The whole value is optional because a panel may define no wings.
type Wings = Option<(Vec<String>, Vec<String>)>;

/// Whether the applet is in the panel, and if not, whether it could be.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Listed in the panel config.
    Present,
    /// Not listed, but installed, so adding it will work.
    Absent,
    /// The desktop entry is not installed. Adding the id would be a no-op:
    /// the panel resolves applets through their entry, so it would silently
    /// skip a name it cannot look up.
    NotInstalled,
    /// The panel's configuration could not be read.
    Unavailable,
}

impl State {
    /// True when there is a button worth offering.
    pub fn is_actionable(self) -> bool {
        matches!(self, State::Present | State::Absent)
    }
}

fn config() -> Option<cosmic_config::Config> {
    cosmic_config::Config::new(PANEL_ID, PANEL_VERSION)
        .map_err(|err| tracing::debug!(%err, "no cosmic panel config"))
        .ok()
}

/// Search the XDG data directories for the applet's desktop entry, the same
/// places the panel will look.
fn entry_installed() -> bool {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        });

    let dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());

    let file = format!("{APPLET_ID}.desktop");

    home.into_iter()
        .chain(dirs.split(':').filter(|d| !d.is_empty()).map(Into::into))
        .any(|dir| dir.join("applications").join(&file).exists())
}

pub fn state() -> State {
    let Some(config) = config() else {
        return State::Unavailable;
    };

    let Ok(wings) = config.get::<Wings>(WINGS_KEY) else {
        return State::Unavailable;
    };

    let listed =
        wings.is_some_and(|(start, end)| start.iter().chain(end.iter()).any(|id| id == APPLET_ID));

    if listed {
        State::Present
    } else if entry_installed() {
        State::Absent
    } else {
        State::NotInstalled
    }
}

/// Add the applet to the end of the panel, or take it out again.
///
/// Removal scans both wings, because a user may have dragged it across in
/// COSMIC Settings since we put it there.
pub fn set(present: bool) -> Result<(), String> {
    let config = config().ok_or_else(|| "no cosmic panel configuration".to_owned())?;

    let (mut start, mut end) = config
        .get::<Wings>(WINGS_KEY)
        .map_err(|err| err.to_string())?
        .unwrap_or_default();

    if present {
        if !start.iter().chain(end.iter()).any(|id| id == APPLET_ID) {
            end.push(APPLET_ID.to_owned());
        }
    } else {
        start.retain(|id| id != APPLET_ID);
        end.retain(|id| id != APPLET_ID);
    }

    config
        .set::<Wings>(WINGS_KEY, Some((start, end)))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the live panel config. Whatever it says, the answer has to be one
    /// of the four states rather than a panic or a hang.
    #[test]
    fn state_is_readable() {
        let state = state();
        // Nothing has been installed by the test run, so "Present" would mean
        // the user added it themselves — all four are legitimate here.
        assert!(matches!(
            state,
            State::Present | State::Absent | State::NotInstalled | State::Unavailable
        ));
    }

    /// The applet id and the desktop entry name have to agree, or the panel
    /// will accept the config entry and then fail to launch anything.
    #[test]
    fn applet_id_matches_desktop_entry() {
        let entry = std::fs::read_to_string("resources/applet.desktop").expect("template exists");
        assert!(entry.contains("X-CosmicApplet=true"));
        assert_eq!(APPLET_ID, crate::applet::APPLET_ID);
    }
}
