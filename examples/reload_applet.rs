// SPDX-License-Identifier: MIT

//! Development utility: make cosmic-panel relaunch the applet.
//!
//! The panel starts an applet once and does not respawn it when it dies, so a
//! rebuilt binary does not take effect until the panel launches it again. It
//! does that when its applet list changes, so removing the entry and putting
//! it straight back is enough.
//!
//! This goes through [`cosmic_tail::panel`] rather than editing the file, so it
//! writes the way cosmic-config does — atomically, via a rename — which is the
//! change the panel is actually watching for.

use std::{thread::sleep, time::Duration};

fn main() {
    cosmic_tail::init_tracing();

    if let Err(err) = cosmic_tail::panel::set(false) {
        eprintln!("could not remove the applet from the panel: {err}");
        return;
    }

    // Give the panel a moment to notice the removal before adding it back;
    // collapsing both writes into one event would leave the list unchanged
    // from its point of view, and nothing would restart.
    sleep(Duration::from_millis(750));

    if let Err(err) = cosmic_tail::panel::set(true) {
        eprintln!("could not add the applet back to the panel: {err}");
        return;
    }

    println!("toggled; the panel should relaunch the applet shortly");
}
