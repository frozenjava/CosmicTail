// SPDX-License-Identifier: MIT

//! The COSMIC panel applet.
//!
//! A separate process from the main window, which means it holds its own
//! `watch-ipn-bus` connection. tailscaled is happy with multiple watchers, and
//! it lets the applet stay current whether or not the window is running.

use cosmic_tail::{applet, i18n, init_tracing};

fn main() -> cosmic::iced::Result {
    init_tracing();

    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    cosmic::applet::run::<applet::Applet>(())
}
