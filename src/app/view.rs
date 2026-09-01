// SPDX-License-Identifier: MIT

//! Rendering for the main window.
//!
//! Split out from [`super`] so the model and the daemon plumbing stay readable;
//! these are all `impl AppModel`, and a child module can see its parent's
//! private fields.

use std::net::IpAddr;

use chrono::Utc;
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;

use super::{AppModel, Message, Page, REPOSITORY};
use crate::fl;
use crate::panel;
use crate::tailscale::{self, BackendState, ConnPath, Device, DeviceStatus, ExitNodeRole};
use crate::ui::{device_row, format, grouping};

/// Fixed width of the left column. Wide enough for the exit-node card's
/// subtitle ("Routing through this exit node") at two lines.
const SIDEBAR_WIDTH: f32 = 220.0;

impl AppModel {
    // -----------------------------------------------------------------
    // Header bar
    // -----------------------------------------------------------------

    /// Show/hide the left pane, in the top-left corner where cosmic-settings
    /// puts the same control. The two icons are COSMIC's own, so this reads as
    /// the same affordance users already know from Settings.
    pub(super) fn sidebar_toggle(&self) -> Element<'_, Message> {
        let icon = if self.panes.sidebar_open() {
            "navbar-open-symbolic"
        } else {
            "navbar-closed-symbolic"
        };

        widget::button::icon(widget::icon::from_name(icon))
            .padding([8, 16])
            .on_press(Message::ToggleSidebar)
            .into()
    }

    /// The connect toggle plus who we are and how we're doing, as in the
    /// top-left of the macOS window.
    pub(super) fn header_state(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();

        let want_running = self.prefs.as_ref().is_some_and(|p| p.want_running);
        let backend = self.tailnet.as_ref().map(|t| &t.backend);

        let label = self
            .tailnet
            .as_ref()
            .map_or_else(|| fl!("bus-connecting"), |t| backend_label(&t.backend));

        let tailnet_name = self
            .tailnet
            .as_ref()
            .and_then(|t| t.tailnet_name.clone())
            .unwrap_or_else(|| fl!("app-title"));

        // Disabled until prefs arrive: without them we do not know which way
        // the switch should be pointing, and a toggle that snaps to the right
        // position a moment after you click it is worse than one that waits.
        let toggler = widget::toggler(want_running)
            .on_toggle_maybe(self.prefs.as_ref().map(|_| Message::SetConnected));

        widget::row::with_capacity(2)
            .push(toggler)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text::body(tailnet_name))
                    .push(widget::text::caption(label).class(match backend {
                        Some(BackendState::Running) => success_text(),
                        _ => dim_text(),
                    })),
            )
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .into()
    }

    // -----------------------------------------------------------------
    // Window body
    // -----------------------------------------------------------------

    pub(super) fn window_view(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let mut column = widget::column::with_capacity(4).spacing(spacing.space_xxs);

        for banner in self.banners() {
            column = column.push(banner);
        }

        let Some(tailnet) = &self.tailnet else {
            return column
                .push(
                    widget::text::body(fl!("tailnet-loading"))
                        .apply(widget::container)
                        .center(Length::Fill),
                )
                .into();
        };

        let mut panes = widget::row::with_capacity(5).height(Length::Fill);

        if self.panes.sidebar_open() {
            panes = panes
                .push(widget::container(self.sidebar(tailnet)).width(Length::Fixed(SIDEBAR_WIDTH)))
                .push(widget::divider::vertical::default());
        }

        panes = panes.push(
            widget::container(self.list_pane(tailnet))
                .width(Length::FillPortion(2))
                .padding(spacing.space_s),
        );

        // In a wide window the detail pane always has a place, showing a hint
        // when nothing is selected. In a narrow one it earns its width only
        // when there is actually something to show.
        if !self.panes.is_narrow() || self.selected().is_some() {
            panes = panes.push(widget::divider::vertical::default()).push(
                widget::container(self.detail_pane(tailnet))
                    .width(Length::FillPortion(3))
                    .padding(spacing.space_s),
            );
        }

        column.push(panes).into()
    }

    /// Failure banners, worst first. Health warnings from tailscaled come last
    /// because they are informational — the tunnel usually still works.
    fn banners(&self) -> Vec<Element<'_, Message>> {
        let mut out = Vec::new();

        if let Some(reason) = &self.write_error {
            out.push(banner(fl!("write-error", reason = reason.as_str()), true));
        }
        if let Some(reason) = &self.status_error {
            out.push(banner(fl!("status-error", reason = reason.as_str()), true));
        }
        if let Some(reason) = &self.bus_error {
            out.push(banner(
                fl!("bus-disconnected", reason = reason.as_str()),
                false,
            ));
        }
        if let Some(tailnet) = &self.tailnet {
            for warning in &tailnet.health {
                out.push(banner(
                    fl!("health-warning", warning = warning.as_str()),
                    false,
                ));
            }
        }

        out
    }

    // -----------------------------------------------------------------
    // Sidebar
    // -----------------------------------------------------------------

    fn sidebar<'a>(&'a self, tailnet: &'a tailscale::TailnetStatus) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        widget::column::with_capacity(3)
            .push(self.exit_node_card(tailnet))
            .push(self.nav_button(Page::Devices, fl!("devices"), "computer-symbolic"))
            .push(self.nav_button(Page::ExitNodes, fl!("exit-nodes"), "network-vpn-symbolic"))
            .spacing(spacing.space_xxs)
            .padding(spacing.space_xs)
            .apply(widget::container)
            // `Container::List` is what `settings::section` draws itself on, so
            // the sidebar sits on the same surface as the settings groups.
            .class(cosmic::theme::Container::List)
            .height(Length::Fill)
            .into()
    }

    /// The card at the top of the sidebar. It shows the active exit node when
    /// there is one, and otherwise tailscaled's recommendation, so the primary
    /// action is one click away in both states.
    fn exit_node_card<'a>(&'a self, tailnet: &'a tailscale::TailnetStatus) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let active = tailnet.active_exit_node();

        // Whether traffic is actually leaving through another machine right
        // now, which is what the accent border below announces.
        let routing = active.is_some();

        let (title, subtitle, action) = match (active, &self.suggestion) {
            (Some(device), _) => (
                device.short_name().to_owned(),
                fl!("exit-node-routing"),
                Some((
                    "media-playback-pause-symbolic",
                    Message::SetExitNode(None),
                    true,
                )),
            ),
            (None, Some(suggestion)) => (
                suggestion.short_name().to_owned(),
                fl!("exit-node-recommended-label"),
                Some((
                    "media-playback-start-symbolic",
                    Message::SetExitNode(Some(suggestion.id.clone())),
                    false,
                )),
            ),
            (None, None) => (fl!("exit-node-none"), fl!("exit-node"), None),
        };

        let mut row = widget::row::with_capacity(2)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text::body(title))
                    .push(widget::text::caption(subtitle).class(dim_text()))
                    .width(Length::Fill),
            )
            .align_y(Alignment::Center)
            .spacing(spacing.space_xxs);

        if let Some((icon, message, active)) = action {
            row = row.push(
                widget::button::icon(widget::icon::from_name(icon))
                    .class(if active {
                        cosmic::theme::Button::Suggested
                    } else {
                        cosmic::theme::Button::Standard
                    })
                    .on_press(message),
            );
        }

        widget::container(row)
            .padding(spacing.space_xs)
            // `Container::Card` resolves to the *same* surface the sidebar sits
            // on — `background.component.base` for both — so the card vanished
            // into it. COSMIC defines a ladder of surfaces for this; stepping
            // the card up to `secondary` puts it one rung above the sidebar,
            // and the divider colour gives it an edge. Both come from the
            // theme, so this follows a re-tint or a light theme.
            .class(cosmic::theme::Container::Custom(Box::new(move |theme| {
                let cosmic = theme.cosmic();
                let mut style = cosmic::theme::Container::secondary(cosmic, theme.transparent);
                style.border = cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_s.into(),
                    // Width stays put across states: only the colour changes,
                    // so the card cannot shift its contents by a pixel when an
                    // exit node comes or goes. That is also how COSMIC signals
                    // "active" elsewhere — accent colour, not extra weight.
                    width: 1.0,
                    color: if routing {
                        cosmic.accent_color().into()
                    } else {
                        cosmic
                            .background(theme.transparent)
                            .component
                            .divider
                            .into()
                    },
                };
                style
            })))
            .width(Length::Fill)
            .into()
    }

    fn nav_button(&self, page: Page, label: String, icon: &'static str) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();

        widget::button::custom(
            widget::row::with_capacity(2)
                .push(widget::icon::from_name(icon).size(16))
                .push(widget::text::body(label))
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center),
        )
        .class(cosmic::theme::Button::ListItem(corner_radii()))
        .selected(self.page == page)
        .width(Length::Fill)
        .on_press(Message::SelectPage(page))
        .into()
    }

    // -----------------------------------------------------------------
    // Centre pane
    // -----------------------------------------------------------------

    fn list_pane<'a>(&'a self, tailnet: &'a tailscale::TailnetStatus) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let title = match self.page {
            Page::Devices => fl!("devices"),
            Page::ExitNodes => fl!("exit-nodes"),
        };

        let body = match self.page {
            Page::Devices => self.devices_list(tailnet),
            Page::ExitNodes => self.exit_nodes_list(tailnet),
        };

        widget::column::with_capacity(3)
            .push(widget::text::title3(title))
            .push(
                widget::search_input(fl!("search-placeholder"), &self.search)
                    .on_input(Message::Search)
                    .on_clear(Message::Search(String::new())),
            )
            .push(widget::scrollable(body).height(Length::Fill))
            .spacing(spacing.space_s)
            .height(Length::Fill)
            .into()
    }

    /// Devices grouped by owner, this account's first, online-first inside
    /// each group.
    fn devices_list<'a>(&'a self, tailnet: &'a tailscale::TailnetStatus) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();
        let mut column = widget::column::with_capacity(8).spacing(spacing.space_xxs);
        let mut shown = 0usize;

        for group in grouping::group_by_owner(tailnet, true) {
            let matching: Vec<&Device> = group
                .devices
                .into_iter()
                .filter(|d| self.matches_search(d))
                .collect();

            // A group whose every device was filtered out should take its
            // header with it, rather than leaving a heading over nothing.
            if matching.is_empty() {
                continue;
            }

            column = column.push(
                widget::text::caption_heading(group.label.to_uppercase())
                    .class(dim_text())
                    .apply(widget::container)
                    .padding([spacing.space_xs, 0, 0, 0]),
            );

            for device in matching {
                shown += 1;
                column = column.push(self.device_button(device));
            }
        }

        if shown == 0 {
            return widget::text::body(fl!("no-devices"))
                .class(dim_text())
                .into();
        }

        column.into()
    }

    fn device_button<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        widget::button::custom(device_row::stacked(device))
            .class(cosmic::theme::Button::ListItem(corner_radii()))
            .selected(self.selected() == Some(&device.id))
            .width(Length::Fill)
            .on_press(Message::SelectDevice(device.id.clone()))
            .into()
    }

    /// The recommendation, then everything advertising exit-node capability.
    fn exit_nodes_list<'a>(
        &'a self,
        tailnet: &'a tailscale::TailnetStatus,
    ) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();
        let mut column = widget::column::with_capacity(8).spacing(spacing.space_xxs);

        if let Some(suggestion) = &self.suggestion {
            column = column
                .push(
                    widget::row::with_capacity(2)
                        .push(widget::text::body(fl!("exit-node-recommended")))
                        .push(
                            widget::text::body(suggestion.short_name().to_owned())
                                .class(dim_text()),
                        )
                        .spacing(spacing.space_xs),
                )
                .push(widget::divider::horizontal::default());
        }

        column = column.push(
            widget::text::caption_heading(fl!("exit-node-available").to_uppercase())
                .class(dim_text()),
        );

        // No "None" row any more: every row now carries its own Use/Stop
        // button, so the active one's "Stop using" clears the exit node. A
        // separate None row would be a third way to do the same thing, and its
        // highlight would mean "no exit node" while every other highlight in
        // this list means "detail pane open" — two meanings, one appearance.
        let mut any = false;
        for device in tailnet.exit_node_candidates() {
            if !self.matches_search(device) {
                continue;
            }
            any = true;

            let active = device.exit_node == ExitNodeRole::Active;

            // Pressing the row opens the detail pane, exactly as in the device
            // list; choosing the node is the button's job. The action sits
            // beside the row rather than inside it because a button nested in a
            // button never receives the press — the outer one swallows it.
            let row = widget::row::with_capacity(2)
                .push(
                    widget::button::custom(device_row::compact(device))
                        .class(cosmic::theme::Button::ListItem(corner_radii()))
                        .selected(self.selected() == Some(&device.id))
                        .width(Length::Fill)
                        .on_press(Message::SelectDevice(device.id.clone())),
                )
                .push(if active {
                    widget::button::standard(fl!("exit-node-stop"))
                        .on_press(Message::SetExitNode(None))
                } else {
                    widget::button::standard(fl!("exit-node-use-short"))
                        .on_press(Message::SetExitNode(Some(device.id.clone())))
                })
                .align_y(Alignment::Center)
                .spacing(spacing.space_xxs);

            column = column.push(row);
        }

        if !any {
            column =
                column.push(widget::text::body(fl!("exit-node-none-available")).class(dim_text()));
        }

        column.into()
    }

    // -----------------------------------------------------------------
    // Detail pane
    // -----------------------------------------------------------------

    fn detail_pane<'a>(&'a self, tailnet: &'a tailscale::TailnetStatus) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let Some(device) = self.selected().and_then(|id| tailnet.device(id)) else {
            return widget::text::body(fl!("select-a-device"))
                .class(dim_text())
                .apply(widget::container)
                .center(Length::Fill)
                .into();
        };

        let now = Utc::now();

        let mut facts = widget::settings::section().add(detail_row(
            fl!("detail-status"),
            format::status_label(device, now),
        ));

        // A device usually carries one of each; naming the family is more use
        // than repeating "IP" twice.
        for ip in &device.ips {
            let label = match ip {
                IpAddr::V4(_) => fl!("detail-ipv4"),
                IpAddr::V6(_) => fl!("detail-ipv6"),
            };
            let ip = ip.to_string();
            let copied = self.copied.shows(&ip);
            facts = facts.add(widget::settings::item::builder(label).control(copyable(ip, copied)));
        }

        if !device.dns_name.is_empty() {
            let copied = self.copied.shows(&device.dns_name);
            facts = facts.add(
                widget::settings::item::builder(fl!("detail-dns"))
                    .control(copyable(device.dns_name.clone(), copied)),
            );
        }

        facts = facts.add(detail_row(fl!("detail-os"), device.os.label().to_owned()));

        if let Some(owner) = &device.owner {
            facts = facts.add(detail_row(fl!("detail-owner"), owner.clone()));
        }

        if device.advertises_routes() {
            facts = facts.add(detail_row(fl!("detail-routes"), device.routes.join(", ")));
        }

        facts = facts.add(detail_row(
            fl!("detail-expiry"),
            match device.expires_in(now) {
                Some(remaining) => fl!("detail-expires-in", days = remaining.num_days()),
                None => fl!("detail-expiry-never"),
            },
        ));

        facts = facts.add(detail_row(fl!("detail-path"), path_label(&device.path)));

        widget::column::with_capacity(4)
            .push(
                widget::row::with_capacity(2)
                    .push(widget::text::title3(device.short_name().to_owned()))
                    .push(device_row::status_dot(&device.status))
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center),
            )
            .push(facts)
            .push(self.detail_actions(device))
            .spacing(spacing.space_s)
            .into()
    }

    /// The exit-node control for the selected device, when it has one.
    fn detail_actions<'a>(&'a self, device: &'a Device) -> Element<'a, Message> {
        // An expired key cannot carry traffic, so offering it would only
        // produce a write that silently does nothing useful.
        let expired = matches!(device.status, DeviceStatus::Expired { .. });

        match device.exit_node {
            ExitNodeRole::Active => widget::button::suggested(fl!("exit-node-stop"))
                .on_press(Message::SetExitNode(None))
                .into(),
            ExitNodeRole::Offered if !expired => widget::button::standard(fl!("exit-node-use"))
                .on_press(Message::SetExitNode(Some(device.id.clone())))
                .into(),
            _ => widget::Space::new().into(),
        }
    }

    // -----------------------------------------------------------------
    // Settings drawer
    // -----------------------------------------------------------------

    /// Add or remove the panel applet.
    ///
    /// The panel is what actually runs the applet — it launches it at login
    /// and keeps it alive — so this is the one place that decides whether
    /// Cosmic Tail has a persistent presence at all.
    fn panel_section(&self) -> Element<'_, Message> {
        let section = widget::settings::section().title(fl!("settings-panel"));

        let item = widget::settings::item::builder(fl!("panel-applet"));

        let row = match self.in_panel {
            panel::State::Present => item.description(fl!("panel-applet-description")).control(
                widget::button::standard(fl!("panel-remove")).on_press(Message::SetInPanel(false)),
            ),
            panel::State::Absent => item.description(fl!("panel-applet-description")).control(
                widget::button::suggested(fl!("panel-add")).on_press(Message::SetInPanel(true)),
            ),
            // Adding the id would be accepted and then quietly do nothing,
            // because the panel resolves applets through their desktop entry.
            panel::State::NotInstalled => item
                .description(fl!("panel-not-installed"))
                .control(widget::Space::new()),
            panel::State::Unavailable => item
                .description(fl!("panel-unavailable"))
                .control(widget::Space::new()),
        };

        section.add(row).into()
    }

    pub(super) fn settings_view(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let mut column = widget::column::with_capacity(4).spacing(spacing.space_s);

        // The panel section comes first and sits above the prefs guard: it is
        // about this app's own installation, not about the daemon, so it stays
        // usable even when tailscaled is unreachable.
        column = column.push(self.panel_section());

        // Every control below writes a pref, so without prefs there is nothing
        // truthful to draw.
        let Some(prefs) = &self.prefs else {
            return column
                .push(widget::text::body(fl!("tailnet-loading")))
                .into();
        };

        column = column.push(
            widget::settings::section()
                .title(fl!("settings-general"))
                .add(
                    widget::settings::item::builder(fl!("settings-allow-incoming"))
                        .description(fl!("settings-allow-incoming-description"))
                        .toggler(!prefs.shields_up, Message::SetAllowIncoming),
                )
                .add(
                    widget::settings::item::builder(fl!("settings-accept-dns"))
                        .toggler(prefs.accept_dns, Message::SetAcceptDns),
                )
                .add(
                    widget::settings::item::builder(fl!("settings-accept-routes"))
                        .description(fl!("settings-accept-routes-description"))
                        .toggler(prefs.accept_routes, Message::SetAcceptRoutes),
                ),
        );

        column = column.push(
            widget::settings::section()
                .title(fl!("exit-nodes"))
                .add(
                    widget::settings::item::builder(fl!("run-exit-node"))
                        .description(fl!("run-exit-node-description"))
                        .toggler(prefs.advertises_exit_node(), Message::SetRunExitNode),
                )
                .add(
                    widget::settings::item::builder(fl!("allow-lan-access"))
                        .description(fl!("allow-lan-access-description"))
                        .toggler(prefs.exit_node_allow_lan_access, Message::SetAllowLanAccess),
                ),
        );

        if let Some(tailnet) = &self.tailnet {
            column = column.push(
                widget::settings::section()
                    .title(fl!("settings-daemon"))
                    .add(detail_row(
                        fl!("settings-version"),
                        tailnet.daemon_version.clone(),
                    ))
                    .add(detail_row(
                        fl!("settings-tailnet"),
                        tailnet.tailnet_name.clone().unwrap_or_default(),
                    ))
                    .add(detail_row(
                        fl!("settings-operator"),
                        prefs.operator_user.clone().unwrap_or_default(),
                    )),
            );
        }

        column.push(about_section()).push(disclaimer()).into()
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    /// Case-insensitive match across the three things you would search a
    /// device by: its name, its MagicDNS name, and its addresses.
    fn matches_search(&self, device: &Device) -> bool {
        let query = self.search.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }

        device.short_name().to_lowercase().contains(&query)
            || device.dns_name.to_lowercase().contains(&query)
            || device.ips.iter().any(|ip| ip.to_string().contains(&query))
    }
}

/// What the standalone About drawer used to say, as a settings section.
///
/// Everything here is fixed at compile time, so it takes no state — and unlike
/// the sections above it does not depend on the daemon being reachable.
fn about_section<'a>() -> Element<'a, Message> {
    widget::settings::section()
        .title(fl!("about"))
        // No icon on this row: the item builder reserves the icon slot whether
        // or not anything renders in it, which left the name indented past
        // every other label in the section.
        .add(
            widget::settings::item::builder(fl!("app-title"))
                .control(widget::text::body(env!("CARGO_PKG_VERSION")).class(dim_text())),
        )
        .add(
            widget::settings::item::builder(fl!("repository")).control(
                // `button::link` is a button with no chrome: accent-coloured
                // text that reads as a hyperlink, still keyboard-focusable and
                // still activated the same way. `trailing_icon` adds
                // libcosmic's own external-link glyph, so it needs nothing from
                // the icon theme.
                widget::button::link(fl!("open"))
                    .trailing_icon(true)
                    // The builder defaults to zero, which butts the icon
                    // straight against the last letter.
                    .spacing(4)
                    .on_press(Message::LaunchUrl(REPOSITORY.to_owned())),
            ),
        )
        .add(detail_row(
            fl!("about-license"),
            env!("CARGO_PKG_LICENSE").to_owned(),
        ))
        .into()
}

/// The third-party notice, deliberately not in a section.
///
/// Sections read as settings — things you act on. This is the fine print, so it
/// sits directly on the pane in muted body text, the way fine print does.
fn disclaimer<'a>() -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    widget::column::with_capacity(2)
        .push(
            widget::text::caption(fl!("about-disclaimer-affiliation"))
                .class(dim_text())
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        )
        .push(
            widget::text::caption(fl!("about-disclaimer-warranty"))
                .class(dim_text())
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        )
        .spacing(spacing.space_xs)
        // Matches the horizontal inset the sections above sit at, so the text
        // lines up with them rather than running to the drawer's edge.
        .padding([0, spacing.space_s])
        .width(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// A value with a copy button, for the fields worth putting on the clipboard.
///
/// While `copied`, the button is replaced by the word itself. Feedback is not
/// a control — there is nothing useful to press in the second after a copy —
/// and a word says what happened without asking anyone to interpret a glyph.
fn copyable<'a>(value: String, copied: bool) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    let control: Element<'a, Message> = if copied {
        widget::text::body(fl!("copied"))
            .class(success_text())
            .into()
    } else {
        widget::button::icon(widget::icon::from_name("edit-copy-symbolic"))
            .on_press(Message::Copy(value.clone()))
            .into()
    };

    widget::row::with_capacity(2)
        .push(widget::text::body(value))
        .push(control)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .into()
}

/// A read-only label/value pair in a settings section.
fn detail_row<'a>(label: String, value: String) -> widget::Row<'a, Message, cosmic::Theme> {
    widget::settings::item::builder(label).control(widget::text::body(value).class(dim_text()))
}

fn banner<'a>(text: String, severe: bool) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    widget::text::body(text)
        .class(if severe { error_text() } else { dim_text() })
        .apply(widget::container)
        .padding([spacing.space_xxs, spacing.space_s])
        .width(Length::Fill)
        .into()
}

fn backend_label(state: &BackendState) -> String {
    match state {
        BackendState::Running => fl!("state-running"),
        BackendState::Stopped => fl!("state-stopped"),
        BackendState::Starting => fl!("state-starting"),
        BackendState::NeedsLogin | BackendState::NeedsMachineAuth => fl!("state-needs-login"),
        other => fl!("state-other", state = format!("{other:?}")),
    }
}

fn path_label(path: &ConnPath) -> String {
    match path {
        ConnPath::Direct { addr } => fl!("path-direct", addr = addr.as_str()),
        ConnPath::Relayed { derp_region } => fl!("path-relayed", region = derp_region.as_str()),
        ConnPath::Unknown => fl!("path-unknown"),
    }
}

/// `Button::ListItem` wants explicit corner radii rather than taking them from
/// the theme itself.
fn corner_radii() -> [f32; 4] {
    cosmic::theme::active().cosmic().corner_radii.radius_s
}

// These take no captures, so they are plain `fn` pointers and cost one theme
// lookup at draw time instead of one per call site.
fn dim_text() -> cosmic::theme::Text {
    cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().palette.neutral_7.into()),
        ..Default::default()
    })
}

fn success_text() -> cosmic::theme::Text {
    cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().success_color().into()),
        ..Default::default()
    })
}

fn error_text() -> cosmic::theme::Text {
    cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().destructive_color().into()),
        ..Default::default()
    })
}
