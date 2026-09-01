// SPDX-License-Identifier: MIT

//! The COSMIC panel applet.
//!
//! A panel popup is a single surface, so the nested submenus of the macOS menu
//! bar item become drill-down pages: pressing a row replaces the popup's
//! contents and leaves a back arrow at the top. [`Page`] is the whole of that
//! navigation state.

use std::collections::HashMap;
use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::applet::{menu_button, padded_control};
use cosmic::iced::core::window;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Rectangle, Subscription};
use cosmic::prelude::*;
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget;

use crate::fl;
use crate::tailscale::{self, BackendState, Device, DeviceId, ExitNodeRole};
use crate::ui::{copy, device_row, format, grouping};

const ICON_CONNECTED: &[u8] =
    include_bytes!("../resources/icons/cosmic-tail-connected-symbolic.svg");
const ICON_DISCONNECTED: &[u8] =
    include_bytes!("../resources/icons/cosmic-tail-disconnected-symbolic.svg");
const ICON_EXIT_NODE: &[u8] =
    include_bytes!("../resources/icons/cosmic-tail-exit-node-symbolic.svg");

/// The applet's identity, shared with [`crate::panel`] because the panel
/// config names applets by exactly this string.
pub const APPLET_ID: &str = "com.github.frozenjava.CosmicTailApplet";

/// Where the popup currently is. The macOS menu nests submenus; a panel popup
/// cannot, so this is a stack one level deep in each direction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Root,
    /// The list of owners.
    NetworkDevices,
    /// One owner's devices. Keyed by the group label rather than an index so a
    /// netmap change between render and click cannot silently re-point it at
    /// somebody else's machines.
    Owner(String),
    ExitNodes,
}

pub struct Applet {
    core: Core,
    popup: Option<Id>,
    page: Page,

    tailscale: tailscale::Client,
    tailnet: Option<tailscale::TailnetStatus>,
    prefs: Option<tailscale::Prefs>,
    suggestion: Option<tailscale::ExitNodeSuggestion>,
    error: Option<String>,
    /// Which value was copied most recently, for the transient tick.
    copied: copy::Feedback,
}

#[derive(Clone, Debug)]
pub enum Message {
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
    Navigate(Page),

    Tailscale(tailscale::Event),
    StatusLoaded(Result<tailscale::TailnetStatus, String>),
    PrefsLoaded(Option<tailscale::Prefs>),
    SuggestionLoaded(Option<tailscale::ExitNodeSuggestion>),

    SetConnected(bool),
    SetExitNode(Option<DeviceId>),
    SetAllowLanAccess(bool),
    SetRunExitNode(bool),
    PrefsWritten(Result<(), String>),

    Copy(String),
    /// Time to check whether the "copied" tick has been up long enough.
    CopyTick,
    OpenWindow,
    /// The replacement window has been launched.
    WindowOpened,
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APPLET_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let applet = Applet {
            core,
            popup: None,
            page: Page::default(),
            tailscale: tailscale::Client::new(),
            tailnet: None,
            prefs: None,
            suggestion: None,
            error: None,
            copied: copy::Feedback::default(),
        };

        let command = applet.refresh();
        (applet, command)
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// The bus runs whether or not the popup is open: the panel icon reports
    /// the connection state, so it has to stay current even when nobody has
    /// the menu open.
    fn subscription(&self) -> Subscription<Message> {
        let bus = tailscale::subscription(self.tailscale.clone()).map(Message::Tailscale);

        // Only runs while a tick is showing, so an idle applet has no timer.
        if self.copied.is_active() {
            Subscription::batch([
                bus,
                cosmic::iced::time::every(copy::TICK).map(|_| Message::CopyTick),
            ])
        } else {
            bus
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }

            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::Navigate(page) => self.page = page,

            Message::Tailscale(event) => match event {
                tailscale::Event::Connected => {
                    self.error = None;
                    return self.refresh();
                }
                tailscale::Event::Disconnected(reason) => self.error = Some(reason),
                tailscale::Event::Notify(_) => return self.refresh(),
            },

            Message::StatusLoaded(Ok(status)) => {
                self.error = None;
                self.tailnet = Some(status);
            }
            Message::StatusLoaded(Err(err)) => self.error = Some(err),
            Message::PrefsLoaded(prefs) => self.prefs = prefs,
            Message::SuggestionLoaded(suggestion) => self.suggestion = suggestion,

            Message::SetConnected(on) => {
                return self.apply(tailscale::PrefsPatch::new().want_running(on));
            }
            Message::SetExitNode(id) => {
                return self.apply(tailscale::PrefsPatch::new().exit_node(id.as_ref()));
            }
            Message::SetAllowLanAccess(on) => {
                return self.apply(tailscale::PrefsPatch::new().exit_node_allow_lan_access(on));
            }
            Message::SetRunExitNode(on) => {
                self.error = None;
                let client = self.tailscale.clone();
                return cosmic::task::future(async move {
                    Message::PrefsWritten(
                        client
                            .set_advertise_exit_node(on)
                            .await
                            .map(|_| ())
                            .map_err(write_error_text),
                    )
                });
            }

            Message::PrefsWritten(Ok(())) => self.error = None,
            Message::PrefsWritten(Err(err)) => self.error = Some(err),

            Message::Copy(text) => {
                self.copied.mark(text.clone());
                return cosmic::iced::clipboard::write(text);
            }

            Message::CopyTick => self.copied.expire(),

            // Wayland has no unminimize request, and a panel applet cannot
            // obtain an activation token to raise another window (both ids the
            // token API accepts are unresolvable from here). So instead of
            // trying to bring the existing window forward, replace it: ask it
            // to quit, then launch a new one. A freshly mapped toplevel is
            // presented by the compositor without needing permission.
            Message::OpenWindow => {
                return cosmic::task::future(async {
                    replace_window().await;
                    Message::WindowOpened
                });
            }

            Message::WindowOpened => {
                // Dismiss the menu, the way picking any other launcher entry
                // would.
                if let Some(id) = self.popup.take() {
                    return cosmic::task::message(cosmic::Action::Cosmic(
                        cosmic::app::Action::Surface(destroy_popup(id)),
                    ));
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let have_popup = self.popup;

        self.core
            .applet
            .icon_button_from_handle(widget::icon::from_svg_bytes(self.panel_icon()).symbolic(true))
            .on_press_with_rectangle(move |offset, bounds| {
                let Some(id) = have_popup else {
                    return Message::Surface(app_popup::<Applet>(
                        |_| Default::default(),
                        move |state: &mut Applet| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);
                            // Always open at the root: a menu that reopens
                            // three levels deep because that is where you left
                            // it is disorienting.
                            state.page = Page::Root;

                            let mut settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            settings
                        },
                        Some(Box::new(|state: &Applet| {
                            Element::from(state.core.applet.popup_container(state.popup_view()))
                                .map(cosmic::Action::App)
                        })),
                    ));
                };

                Message::Surface(destroy_popup(id))
            })
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        self.core.applet.popup_container(self.popup_view()).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Applet {
    /// Re-read everything the popup and the panel icon draw from.
    fn refresh(&self) -> Task<Message> {
        let status = {
            let client = self.tailscale.clone();
            cosmic::task::future(async move {
                Message::StatusLoaded(client.status().await.map_err(|err| err.to_string()))
            })
        };
        let prefs = {
            let client = self.tailscale.clone();
            cosmic::task::future(async move { Message::PrefsLoaded(client.prefs().await.ok()) })
        };
        let suggestion = {
            let client = self.tailscale.clone();
            cosmic::task::future(async move {
                Message::SuggestionLoaded(client.suggest_exit_node().await.ok().flatten())
            })
        };

        Task::batch([status, prefs, suggestion])
    }

    fn apply(&mut self, patch: tailscale::PrefsPatch) -> Task<Message> {
        self.error = None;
        let client = self.tailscale.clone();
        cosmic::task::future(async move {
            Message::PrefsWritten(
                client
                    .set_prefs(patch)
                    .await
                    .map(|_| ())
                    .map_err(write_error_text),
            )
        })
    }

    /// Three states worth distinguishing at a glance in the panel: off, on, and
    /// on-but-routing-elsewhere.
    fn panel_icon(&self) -> &'static [u8] {
        let Some(tailnet) = &self.tailnet else {
            return ICON_DISCONNECTED;
        };

        if tailnet.backend != BackendState::Running {
            ICON_DISCONNECTED
        } else if tailnet.exit_node.is_some() {
            ICON_EXIT_NODE
        } else {
            ICON_CONNECTED
        }
    }

    // -----------------------------------------------------------------
    // Popup
    // -----------------------------------------------------------------

    fn popup_view(&self) -> Element<'_, Message> {
        match &self.page {
            Page::Root => self.root_page(),
            Page::NetworkDevices => self.owners_page(),
            Page::Owner(label) => self.owner_page(label),
            Page::ExitNodes => self.exit_nodes_page(),
        }
    }

    fn root_page(&self) -> Element<'_, Message> {
        let mut column = widget::column::with_capacity(10).padding([8, 0]);

        // Header: name, state, and the connect switch.
        let want_running = self.prefs.as_ref().is_some_and(|p| p.want_running);
        let state_label = self
            .tailnet
            .as_ref()
            .map_or_else(|| fl!("bus-connecting"), |t| backend_label(&t.backend));

        column = column.push(padded_control(
            widget::row::with_capacity(2)
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text::body(fl!("app-title")))
                        .push(widget::text::caption(state_label).class(dim_text()))
                        .width(Length::Fill),
                )
                .push(
                    widget::toggler(want_running)
                        .on_toggle_maybe(self.prefs.as_ref().map(|_| Message::SetConnected)),
                )
                .align_y(Alignment::Center),
        ));

        // The active exit node belongs with the connection state rather than
        // with the device lists: it changes where all your traffic goes, so it
        // reads as part of "what is this machine currently doing".
        if let Some(exit) = self
            .tailnet
            .as_ref()
            .and_then(tailscale::TailnetStatus::active_exit_node)
        {
            column = column.push(padded_control(
                widget::row::with_capacity(2)
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text::caption(fl!("exit-node")).class(dim_text()))
                            .push(widget::text::body(exit.short_name().to_owned()))
                            .width(Length::Fill),
                    )
                    .push(
                        widget::button::standard(fl!("exit-node-stop"))
                            .on_press(Message::SetExitNode(None)),
                    )
                    .align_y(Alignment::Center),
            ));
        }

        column = column.push(padded_control(widget::divider::horizontal::default()));

        if let Some(tailnet) = &self.tailnet {
            if let Some(name) = &tailnet.tailnet_name {
                column = column.push(padded_control(
                    widget::text::body(name.clone()).class(dim_text()),
                ));
            }

            // Clicking this device copies its address, the way the macOS menu
            // does — it is the single most common reason to open this menu.
            if let Some(this) = &tailnet.self_device {
                let ip = format::primary_ip(this);
                let label = widget::text::body(fl!(
                    "this-device",
                    name = this.short_name(),
                    ip = ip.as_str()
                ));
                column = column.push(copy_row(label.into(), ip, &self.copied));
            }
        }

        column = column
            .push(self.drill_row(fl!("network-devices"), Page::NetworkDevices))
            .push(padded_control(widget::divider::horizontal::default()))
            .push(self.drill_row(fl!("exit-nodes"), Page::ExitNodes))
            .push(padded_control(widget::divider::horizontal::default()))
            .push(
                menu_button(widget::text::body(fl!("open-window"))).on_press(Message::OpenWindow),
            );

        if let Some(error) = &self.error {
            column = column.push(padded_control(
                widget::text::caption(error.clone()).class(error_text()),
            ));
        }

        column.into()
    }

    fn owners_page(&self) -> Element<'_, Message> {
        let mut column = widget::column::with_capacity(8)
            .padding([8, 0])
            .push(self.back_row(fl!("network-devices"), Page::Root))
            .push(padded_control(widget::divider::horizontal::default()));

        let Some(tailnet) = &self.tailnet else {
            return column
                .push(padded_control(widget::text::body(fl!("tailnet-loading"))))
                .into();
        };

        // `false`: this machine already has its own line on the root page.
        for group in grouping::group_by_owner(tailnet, false) {
            column =
                column.push(self.drill_row(group.label.clone(), Page::Owner(group.label.clone())));
        }

        column.into()
    }

    fn owner_page(&self, label: &str) -> Element<'_, Message> {
        let column = widget::column::with_capacity(4)
            .padding([8, 0])
            .push(self.back_row(label.to_owned(), Page::NetworkDevices))
            .push(padded_control(widget::divider::horizontal::default()));

        let Some(tailnet) = &self.tailnet else {
            return column
                .push(padded_control(widget::text::body(fl!("tailnet-loading"))))
                .into();
        };

        // The group can vanish between opening it and the next netmap update,
        // in which case an empty list is the honest thing to show.
        let group = grouping::group_by_owner(tailnet, false)
            .into_iter()
            .find(|g| g.label == label);

        let mut body = widget::column::with_capacity(24);
        match group {
            Some(group) => {
                for device in group.devices {
                    let ip = format::primary_ip(device);
                    body = body.push(copy_row(device_row::compact(device), ip, &self.copied));
                }
            }
            None => {
                body = body.push(padded_control(
                    widget::text::body(fl!("no-devices")).class(dim_text()),
                ));
            }
        }

        // `popup_container` caps the popup at 1000px tall; a tailnet with more
        // devices than that has to scroll rather than be clipped.
        column
            .push(widget::scrollable(body).height(Length::Shrink))
            .into()
    }

    fn exit_nodes_page(&self) -> Element<'_, Message> {
        let mut column = widget::column::with_capacity(16)
            .padding([8, 0])
            .push(self.back_row(fl!("exit-nodes"), Page::Root))
            .push(padded_control(widget::divider::horizontal::default()));

        let Some(tailnet) = &self.tailnet else {
            return column
                .push(padded_control(widget::text::body(fl!("tailnet-loading"))))
                .into();
        };

        column = column.push(check_row(
            fl!("exit-node-none"),
            tailnet.exit_node.is_none(),
            Message::SetExitNode(None),
        ));

        if let Some(suggestion) = &self.suggestion {
            column = column.push(
                menu_button(widget::text::body(format!(
                    "{}: {}",
                    fl!("exit-node-recommended"),
                    suggestion.short_name()
                )))
                .on_press(Message::SetExitNode(Some(suggestion.id.clone()))),
            );
        }

        column = column
            .push(padded_control(widget::divider::horizontal::default()))
            .push(padded_control(
                widget::text::caption_heading(fl!("exit-node-available")).class(dim_text()),
            ));

        let mut any = false;
        for device in tailnet.exit_node_candidates() {
            any = true;
            column = column.push(exit_node_row(device));
        }

        if !any {
            column = column.push(padded_control(
                widget::text::body(fl!("exit-node-none-available")).class(dim_text()),
            ));
        }

        // The two exit-node preferences, in the same place the macOS menu puts
        // them.
        if let Some(prefs) = &self.prefs {
            column = column
                .push(padded_control(widget::divider::horizontal::default()))
                .push(check_row(
                    fl!("allow-lan-access"),
                    prefs.exit_node_allow_lan_access,
                    Message::SetAllowLanAccess(!prefs.exit_node_allow_lan_access),
                ))
                .push(check_row(
                    fl!("run-exit-node"),
                    prefs.advertises_exit_node(),
                    Message::SetRunExitNode(!prefs.advertises_exit_node()),
                ));
        }

        column.into()
    }

    /// A row that opens a sub-page, with the chevron the macOS menu uses.
    fn drill_row(&self, label: String, page: Page) -> Element<'_, Message> {
        menu_button(
            widget::row::with_capacity(2)
                .push(widget::text::body(label).width(Length::Fill))
                .push(widget::icon::from_name("go-next-symbolic").size(16))
                .align_y(Alignment::Center),
        )
        .on_press(Message::Navigate(page))
        .into()
    }

    /// The header of a sub-page: a back chevron and where you are.
    fn back_row(&self, label: String, to: Page) -> Element<'_, Message> {
        menu_button(
            widget::row::with_capacity(2)
                .push(widget::icon::from_name("go-previous-symbolic").size(16))
                .push(widget::text::body(label))
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .on_press(Message::Navigate(to))
        .into()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// A row whose press copies `value`, showing a tick for a moment afterwards.
///
/// The clipboard gives no feedback of its own, so without this a press looks
/// exactly like a press that did nothing.
fn copy_row<'a>(
    content: Element<'a, Message>,
    value: String,
    copied: &copy::Feedback,
) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(2)
        .push(widget::container(content).width(Length::Fill))
        .align_y(Alignment::Center);

    if copied.shows(&value) {
        row = row.push(widget::text::caption(fl!("copied")).class(success_text()));
    }

    menu_button(row).on_press(Message::Copy(value)).into()
}

/// A row that carries a tick when it is the current choice.
fn check_row<'a>(label: String, checked: bool, message: Message) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(2)
        .push(widget::text::body(label).width(Length::Fill))
        .align_y(Alignment::Center);

    if checked {
        row = row.push(widget::icon::from_name("object-select-symbolic").size(16));
    }

    menu_button(row).on_press(message).into()
}

fn exit_node_row(device: &Device) -> Element<'_, Message> {
    let mut row = widget::row::with_capacity(2)
        .push(widget::container(device_row::compact::<Message>(device)).width(Length::Fill))
        .align_y(Alignment::Center);

    if device.exit_node == ExitNodeRole::Active {
        row = row.push(widget::icon::from_name("object-select-symbolic").size(16));
    }

    menu_button(row)
        .on_press(Message::SetExitNode(Some(device.id.clone())))
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

fn write_error_text(err: tailscale::Error) -> String {
    if err.is_permission_denied() {
        fl!("write-denied")
    } else {
        err.to_string()
    }
}

/// Close any running window and start a fresh one in its place.
///
/// The quit is best-effort: if no window is running the DBus call simply fails
/// with `ServiceUnknown`, which is the normal case and not worth reporting.
async fn replace_window() {
    if quit_running_window().await {
        // The new process claims the same DBus name, so it has to wait for the
        // old one to let go of it — otherwise single-instance sees the corpse,
        // hands off to it, and no window ever appears.
        wait_for_name_release().await;
    }

    launch_window();
}

/// Ask a running window to exit. Returns whether one answered.
async fn quit_running_window() -> bool {
    let Ok(connection) = zbus::Connection::session().await else {
        tracing::warn!("no session bus; cannot replace a running window");
        return false;
    };

    let result = connection
        .call_method(
            Some(crate::app::WINDOW_ID),
            window_object_path().as_str(),
            Some("org.freedesktop.DbusActivation"),
            "ActivateAction",
            &(
                crate::app::QUIT_ACTION,
                Vec::<&str>::new(),
                HashMap::<&str, zbus::zvariant::Value<'_>>::new(),
            ),
        )
        .await;

    match result {
        Ok(_) => true,
        // Nothing was running. This is the common case, not a failure.
        Err(err) => {
            tracing::debug!(%err, "no running window to replace");
            false
        }
    }
}

/// Wait for the window's DBus name to become unowned, up to a short deadline.
async fn wait_for_name_release() {
    let Ok(connection) = zbus::Connection::session().await else {
        return;
    };

    let deadline = std::time::Instant::now() + Duration::from_secs(2);

    while std::time::Instant::now() < deadline {
        let owned = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(crate::app::WINDOW_ID,),
            )
            .await
            .ok()
            .and_then(|reply| reply.body().deserialize::<bool>().ok())
            .unwrap_or(false);

        if !owned {
            return;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    tracing::warn!("the old window did not exit; the new one may hand off to it instead");
}

/// The object path libcosmic serves the activation interface on, which it
/// derives from the app id.
fn window_object_path() -> String {
    format!("/{}", crate::app::WINDOW_ID.replace('.', "/"))
}

/// Start the main window.
///
/// Prefers the binary sitting next to this one so a development build opens
/// the development window rather than an installed copy, falling back to
/// `PATH` for an installed applet.
fn launch_window() {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("cosmic_tail")))
        .filter(|path| path.exists());

    let program = sibling.unwrap_or_else(|| std::path::PathBuf::from("cosmic_tail"));

    match std::process::Command::new(&program).spawn() {
        // Nothing waits on the child, so without this it stays a zombie until
        // the applet itself exits — and the applet lives for the whole session,
        // so they would accumulate one per launch. The thread costs nothing:
        // it blocks in `wait` for the window's lifetime, then goes away.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => {
            tracing::error!(%err, program = %program.display(), "could not open the window");
        }
    }
}

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
