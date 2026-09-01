// SPDX-License-Identifier: MIT

//! The main window. See `bin/cosmic_tail_applet.rs` for the panel applet.

use cosmic_tail::{app, i18n, init_tracing};

fn main() -> cosmic::iced::Result {
    init_tracing();

    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Settings for configuring the application window and iced runtime.
    // The minimum is deliberately below `THREE_PANE_MIN_WIDTH`: narrower than
    // that the window drops to two panes rather than refusing to shrink.
    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(
            app::DEFAULT_WIDTH,
            app::DEFAULT_HEIGHT,
        ))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(480.0)
                .min_height(400.0),
        );

    // Single instance rather than plain `run`: launching a second copy — which
    // is exactly what the applet's "Open Cosmic Tail" does — hands off to the
    // running process over DBus and raises its window, instead of opening a
    // duplicate.
    cosmic::app::run_single_instance::<app::AppModel>(settings, app::Flags)
}
