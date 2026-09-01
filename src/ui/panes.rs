// SPDX-License-Identifier: MIT

//! Which side panes fit, and what has to give way when they don't.
//!
//! Below [`THREE_PANE_MIN_WIDTH`] the sidebar, the device list and the detail
//! pane cannot sit side by side without squeezing all three, so at most one of
//! the sidebar and the detail pane shows. Keeping that in one place — rather
//! than spread across three `update` arms — is what makes it testable.

/// Below this the window drops to two panes. Roughly the sidebar (220) plus a
/// usable list and a usable detail pane, with the dividers and padding between
/// them.
pub const THREE_PANE_MIN_WIDTH: f32 = 860.0;

/// What the caller must do to the detail pane after a layout change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Nothing to do.
    Keep,
    /// The sidebar took its place; clear the selection.
    Close,
}

#[derive(Debug, Clone, Copy)]
pub struct Panes {
    width: f32,
    sidebar_open: bool,
}

impl Panes {
    pub fn new(width: f32) -> Self {
        Self {
            width,
            // A window that starts wide enough starts with the sidebar out.
            sidebar_open: width >= THREE_PANE_MIN_WIDTH,
        }
    }

    pub fn is_narrow(self) -> bool {
        self.width < THREE_PANE_MIN_WIDTH
    }

    pub fn sidebar_open(self) -> bool {
        self.sidebar_open
    }

    /// The user pressed the toggle in the header.
    pub fn toggle_sidebar(&mut self) -> Detail {
        self.sidebar_open = !self.sidebar_open;

        if self.sidebar_open && self.is_narrow() {
            Detail::Close
        } else {
            Detail::Keep
        }
    }

    /// A device was selected, so the detail pane is about to appear.
    pub fn detail_opened(&mut self) {
        if self.is_narrow() {
            self.sidebar_open = false;
        }
    }

    /// The window was resized.
    ///
    /// Only the crossing between wide and narrow moves the sidebar. Reacting to
    /// every resize would spring a deliberately collapsed sidebar back open the
    /// moment the window was nudged.
    pub fn resized(&mut self, width: f32) {
        let was_narrow = self.is_narrow();
        self.width = width;

        if was_narrow != self.is_narrow() {
            self.sidebar_open = !self.is_narrow();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: f32 = 1200.0;
    const NARROW: f32 = 640.0;

    #[test]
    fn starts_open_when_wide_and_closed_when_narrow() {
        assert!(Panes::new(WIDE).sidebar_open());
        assert!(!Panes::new(NARROW).sidebar_open());
    }

    #[test]
    fn narrowing_collapses_the_sidebar_and_widening_restores_it() {
        let mut panes = Panes::new(WIDE);

        panes.resized(NARROW);
        assert!(!panes.sidebar_open());

        panes.resized(WIDE);
        assert!(panes.sidebar_open());
    }

    /// A sidebar the user closed by hand must stay closed while the window is
    /// merely resized within the same regime.
    #[test]
    fn resizing_within_a_regime_leaves_a_manual_choice_alone() {
        let mut panes = Panes::new(WIDE);
        assert_eq!(panes.toggle_sidebar(), Detail::Keep);
        assert!(!panes.sidebar_open());

        panes.resized(WIDE + 200.0);
        assert!(!panes.sidebar_open(), "a wider window reopened it");

        panes.resized(WIDE - 100.0);
        assert!(!panes.sidebar_open(), "a narrower window reopened it");
    }

    /// The two rules that trade one pane for the other, in both directions.
    #[test]
    fn narrow_windows_show_one_side_pane_at_a_time() {
        let mut panes = Panes::new(NARROW);

        // Opening the sidebar evicts the detail pane.
        assert_eq!(panes.toggle_sidebar(), Detail::Close);
        assert!(panes.sidebar_open());

        // Opening details evicts the sidebar.
        panes.detail_opened();
        assert!(!panes.sidebar_open());
    }

    /// Wide windows never make that trade — all three fit.
    #[test]
    fn wide_windows_keep_both_side_panes() {
        let mut panes = Panes::new(WIDE);

        panes.detail_opened();
        assert!(panes.sidebar_open(), "the sidebar closed in a wide window");

        assert_eq!(panes.toggle_sidebar(), Detail::Keep);
        assert!(!panes.sidebar_open());
        assert_eq!(panes.toggle_sidebar(), Detail::Keep);
        assert!(panes.sidebar_open());
    }

    /// The boundary itself counts as wide, so a window sized exactly to the
    /// minimum shows what the minimum is for.
    #[test]
    fn the_boundary_is_wide_enough() {
        assert!(!Panes::new(THREE_PANE_MIN_WIDTH).is_narrow());
        assert!(Panes::new(THREE_PANE_MIN_WIDTH - 1.0).is_narrow());
    }
}
