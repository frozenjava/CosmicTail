// SPDX-License-Identifier: MIT

//! The main window: a three-pane browser over the tailnet.
//!
//! Rendering lives in [`view`]; this file is the model, the messages, and the
//! `update` that ties them to the daemon.

mod view;

use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Subscription;
use cosmic::prelude::*;
use cosmic::widget;

use crate::config::Config;
use crate::fl;
use crate::ui::copy;
use crate::ui::panes::{Detail, Panes};
use crate::{panel, tailscale};

/// The window's identity. Shared with [`crate::applet`], which addresses the
/// running window over DBus using this name and the object path derived from it.
pub const WINDOW_ID: &str = "com.github.frozenjava.CosmicTail";

/// DBus action asking a running window to close so a fresh one can replace it.
///
/// Wayland has no unminimize request and the applet cannot obtain an activation
/// token, so a minimized window genuinely cannot be brought back. Replacing it
/// with a newly mapped one is the only thing that reliably puts a window in
/// front of you, and a newly mapped toplevel is presented by the compositor
/// without needing anyone's permission.
pub const QUIT_ACTION: &str = "quit";

/// Flags for [`cosmic::app::run_single_instance`].
///
/// The window takes no arguments, but single-instance activation is defined in
/// terms of a flags type, so this is an empty one. Its only job is to let a
/// second launch hand off to the running process over DBus instead of opening
/// a duplicate window.
#[derive(Clone, Debug, Default)]
pub struct Flags;

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = String;
    type Args = Vec<String>;
}

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Default window size. Also the width assumed until the first resize event,
/// so the responsive rules below start from something sane.
pub const DEFAULT_WIDTH: f32 = 1000.0;
pub const DEFAULT_HEIGHT: f32 = 700.0;

/// Which list the centre pane is showing.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Devices,
    ExitNodes,
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    /// Configuration data that persists between application runs.
    config: Config,

    /// Talks to the local tailscaled.
    tailscale: tailscale::Client,
    /// Latest `/status` snapshot; `None` until the first fetch lands.
    tailnet: Option<tailscale::TailnetStatus>,
    /// Latest `/prefs`. Held separately because `/status` cannot answer
    /// "is *this* device advertising as an exit node?" — that lives in
    /// `AdvertiseRoutes`, which only prefs carries.
    prefs: Option<tailscale::Prefs>,
    /// tailscaled's own exit-node recommendation, if it has one.
    suggestion: Option<tailscale::ExitNodeSuggestion>,

    /// Which list the centre pane shows.
    page: Page,
    /// The device whose details fill the right pane, remembered per list.
    ///
    /// The two lists answer different questions — "what is this machine?" and
    /// "should I route through it?" — so a selection made in one has no
    /// business following you into the other.
    selected_device: Option<tailscale::DeviceId>,
    selected_exit_node: Option<tailscale::DeviceId>,
    /// Contents of the search field.
    search: String,
    /// Which side panes fit at the current window width. See [`Panes`].
    panes: Panes,
    /// Which value was copied most recently, for the transient tick.
    copied: copy::Feedback,

    /// Whether the applet is in the COSMIC panel. Cached rather than read per
    /// render, because answering it touches the filesystem.
    in_panel: panel::State,

    /// Why the bus is not connected, if it isn't.
    bus_error: Option<String>,
    /// Why the last `/status` fetch failed, if it did.
    status_error: Option<String>,
    /// Why the last prefs write failed, if it did.
    write_error: Option<String>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    LaunchUrl(String),

    /// A notification from the tailscaled bus.
    Tailscale(tailscale::Event),
    /// The result of a `/status` fetch. `Err` carries an already-formatted
    /// message because `tailscale::Error` is not `Clone`.
    StatusLoaded(Result<tailscale::TailnetStatus, String>),
    /// The result of a `/prefs` fetch. A failure here is not worth a banner —
    /// it only costs us the settings controls, and `status_error` will already
    /// be showing if the daemon is genuinely unreachable.
    PrefsLoaded(Option<tailscale::Prefs>),
    /// tailscaled's exit-node recommendation. Absent is normal.
    SuggestionLoaded(Option<tailscale::ExitNodeSuggestion>),

    SelectPage(Page),
    SelectDevice(tailscale::DeviceId),
    /// Show or hide the left pane.
    ToggleSidebar,
    /// The window was resized; carries the new width.
    WindowResized(f32),
    Search(String),
    /// Put text on the system clipboard.
    Copy(String),
    /// Time to check whether the "copied" tick has been up long enough.
    CopyTick,

    /// Connect or disconnect this device.
    SetConnected(bool),
    /// Select an exit node, or `None` to stop using one.
    SetExitNode(Option<tailscale::DeviceId>),
    SetAllowLanAccess(bool),
    /// Advertise this device as an exit node.
    SetRunExitNode(bool),
    SetAcceptDns(bool),
    SetAcceptRoutes(bool),
    /// The inverse of `ShieldsUp`, phrased the way the UI asks it.
    SetAllowIncoming(bool),
    /// Outcome of a prefs write.
    PrefsWritten(Result<(), String>),

    /// Add or remove the applet from the COSMIC panel.
    SetInPanel(bool),

    /// Show or hide the settings drawer.
    ToggleSettings,
    UpdateConfig(Config),
}

/// Turn a write failure into something worth showing a user. Permission denial
/// is by far the most likely one and has an actionable fix, so it gets its own
/// message instead of tailscaled's prose.
fn write_error_text(err: tailscale::Error) -> String {
    if err.is_permission_denied() {
        fl!("write-denied")
    } else {
        err.to_string()
    }
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = Flags;

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = WINDOW_ID;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        // Come back where the last window left off. See `save_ui_state`.
        let selected_device = config.selected_device.clone().map(tailscale::DeviceId);
        let selected_exit_node = config.selected_exit_node.clone().map(tailscale::DeviceId);
        let page = if config.exit_nodes_page {
            Page::ExitNodes
        } else {
            Page::Devices
        };

        let mut app = AppModel {
            core,
            config,

            tailscale: tailscale::Client::new(),
            tailnet: None,
            prefs: None,
            suggestion: None,

            page,
            selected_device,
            selected_exit_node,
            search: String::new(),
            panes: Panes::new(DEFAULT_WIDTH),
            copied: copy::Feedback::default(),
            in_panel: panel::state(),

            bus_error: Some(fl!("bus-connecting")),
            status_error: None,
            write_error: None,
        };

        let command = Task::batch([app.update_title(), app.refresh()]);

        (app, command)
    }

    /// Elements to pack at the start of the header bar.
    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![self.sidebar_toggle(), self.header_state()]
    }

    /// Elements to pack at the end of the header bar.
    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        vec![
            widget::button::icon(widget::icon::from_name("preferences-system-symbolic"))
                .on_press(Message::ToggleSettings)
                .into(),
        ]
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(
            context_drawer::context_drawer(self.settings_view(), Message::ToggleSettings)
                .title(fl!("settings")),
        )
    }

    /// Describes the interface based on the current state of the application model.
    fn view(&self) -> Element<'_, Self::Message> {
        self.window_view()
    }

    /// Register subscriptions for this application.
    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            // The tailscaled notification bus. Its identity is the `Client`, so
            // this starts once and survives every re-render.
            tailscale::subscription(self.tailscale.clone()).map(Message::Tailscale),
        ];

        // Drives the responsive layout. Cheap: iced already routes these
        // events, this only filters them.
        subscriptions.push(
            cosmic::iced::window::resize_events()
                .map(|(_, size)| Message::WindowResized(size.width)),
        );

        // Only runs while a tick is actually showing, so the idle app has no
        // timer at all.
        if self.copied.is_active() {
            subscriptions.push(cosmic::iced::time::every(copy::TICK).map(|_| Message::CopyTick));
        }

        Subscription::batch(subscriptions)
    }

    /// Something asked this already-running instance to come to the front.
    ///
    /// A request carrying an activation token never reaches here: libcosmic
    /// intercepts it and unminimizes, then activates with the token. This is
    /// the fallback for one that arrived without a token — launching
    /// `cosmic_tail` from a terminal, say — which would otherwise do nothing
    /// visible at all. Wayland is likely to ignore both of these (it has no
    /// unminimize request, and focus cannot be taken without a token), but
    /// they cost nothing and are correct elsewhere.
    fn dbus_activation(
        &mut self,
        msg: cosmic::dbus_activation::Message,
    ) -> Task<cosmic::Action<Self::Message>> {
        // The applet asks us to stand down so it can open a window that the
        // compositor will actually show. Save first: the replacement should
        // come up on the same device you were looking at.
        if let cosmic::dbus_activation::Details::ActivateAction { action, .. } = &msg.msg
            && action == QUIT_ACTION
        {
            self.save_ui_state();

            // Not `iced::exit()`: in this version it panics on the way out with
            // "`async fn` resumed after completion" (iced/winit/src/lib.rs:765),
            // because the event loop resumes the run future after it has
            // finished. The exit code would be 101 and the backtrace would go
            // to the journal on every single relaunch.
            //
            // Exiting here skips destructors, which is safe precisely because
            // `save_ui_state` above already wrote synchronously — nothing else
            // in this process owns state that outlives it.
            std::process::exit(0);
        }

        let Some(id) = self.core.main_window_id() else {
            return Task::none();
        };

        cosmic::iced::window::minimize(id, false).chain(cosmic::iced::window::gain_focus(id))
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tailscale(event) => match event {
                // The bus reports changes, not state. Anything that happened
                // while disconnected is invisible, so resync from scratch.
                tailscale::Event::Connected => {
                    self.bus_error = None;
                    return self.refresh();
                }
                tailscale::Event::Disconnected(reason) => self.bus_error = Some(reason),
                // `watch.rs` already drops empty notifications, so anything
                // arriving here changed something. Re-fetching is cheaper to
                // reason about than folding deltas in by hand, and a netmap
                // change requires `/status` regardless.
                tailscale::Event::Notify(_) => return self.refresh(),
            },

            Message::StatusLoaded(Ok(status)) => {
                self.status_error = None;
                // A device can leave the tailnet while its detail pane is
                // open, in either list.
                let still_present = |id: &tailscale::DeviceId| {
                    status
                        .self_device
                        .iter()
                        .chain(status.devices.iter())
                        .any(|d| &d.id == id)
                };
                self.selected_device.take_if(|id| !still_present(id));
                self.selected_exit_node.take_if(|id| !still_present(id));
                self.tailnet = Some(status);
            }

            Message::StatusLoaded(Err(err)) => self.status_error = Some(err),

            Message::PrefsLoaded(prefs) => self.prefs = prefs,

            Message::SuggestionLoaded(suggestion) => self.suggestion = suggestion,

            Message::SelectPage(page) => self.page = page,

            Message::SelectDevice(id) => {
                // Clicking the open device closes the detail pane, so the list
                // can be given the full width again without a separate control.
                let slot = self.selected_mut();
                *slot = if slot.as_ref() == Some(&id) {
                    None
                } else {
                    Some(id)
                };

                if self.selected().is_some() {
                    self.panes.detail_opened();
                }
            }

            Message::ToggleSidebar => {
                if self.panes.toggle_sidebar() == Detail::Close {
                    *self.selected_mut() = None;
                }
            }

            Message::WindowResized(width) => self.panes.resized(width),

            Message::Search(query) => self.search = query,

            Message::Copy(text) => {
                self.copied.mark(text.clone());
                return cosmic::iced::clipboard::write(text);
            }

            Message::CopyTick => self.copied.expire(),

            Message::SetConnected(on) => {
                return self.apply(tailscale::PrefsPatch::new().want_running(on));
            }

            Message::SetExitNode(id) => {
                return self.apply(tailscale::PrefsPatch::new().exit_node(id.as_ref()));
            }

            Message::SetAllowLanAccess(on) => {
                return self.apply(tailscale::PrefsPatch::new().exit_node_allow_lan_access(on));
            }

            Message::SetAcceptDns(on) => {
                return self.apply(tailscale::PrefsPatch::new().accept_dns(on));
            }

            Message::SetAcceptRoutes(on) => {
                return self.apply(tailscale::PrefsPatch::new().accept_routes(on));
            }

            // `ShieldsUp` blocks incoming connections, so the checkbox reads
            // as its inverse. Flipping it here keeps the negation in one place.
            Message::SetAllowIncoming(on) => {
                return self.apply(tailscale::PrefsPatch::new().shields_up(!on));
            }

            // Not a plain patch: `AdvertiseRoutes` is replaced wholesale, so
            // the client has to read the current list and merge.
            Message::SetRunExitNode(on) => {
                self.write_error = None;
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

            // Not a daemon write at all: this edits cosmic-panel's own config,
            // which the panel reloads on change.
            Message::SetInPanel(present) => {
                match panel::set(present) {
                    Ok(()) => self.write_error = None,
                    Err(err) => self.write_error = Some(err),
                }
                self.in_panel = panel::state();
            }

            Message::PrefsWritten(Ok(())) => self.write_error = None,

            Message::PrefsWritten(Err(err)) => self.write_error = Some(err),

            Message::ToggleSettings => {
                self.core.window.show_context = !self.core.window.show_context;
            }

            Message::UpdateConfig(config) => self.config = config,

            Message::LaunchUrl(url) => {
                if let Err(err) = open::that_detached(&url) {
                    tracing::error!(%err, %url, "failed to open url");
                }
            }
        }

        Task::none()
    }
}

impl AppModel {
    /// Re-read everything the UI draws from.
    ///
    /// The three calls are independent, so they run concurrently. Each closure
    /// must be `'static`, hence its own cloned `Client` rather than a borrow
    /// of `self`.
    fn refresh(&self) -> Task<cosmic::Action<Message>> {
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

    /// Send a prefs patch, clearing any previous failure first.
    ///
    /// The result is discarded on success: the write pushes a bus notification,
    /// which triggers [`AppModel::refresh`], so applying the returned prefs here
    /// too would just race with that.
    fn apply(&mut self, patch: tailscale::PrefsPatch) -> Task<cosmic::Action<Message>> {
        self.write_error = None;
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

    /// The selection belonging to the list currently on screen.
    pub(super) fn selected(&self) -> Option<&tailscale::DeviceId> {
        match self.page {
            Page::Devices => self.selected_device.as_ref(),
            Page::ExitNodes => self.selected_exit_node.as_ref(),
        }
    }

    fn selected_mut(&mut self) -> &mut Option<tailscale::DeviceId> {
        match self.page {
            Page::Devices => &mut self.selected_device,
            Page::ExitNodes => &mut self.selected_exit_node,
        }
    }

    /// Write the bits of view state worth surviving a relaunch.
    ///
    /// Best-effort: failing to persist which row was highlighted is not worth
    /// interrupting a shutdown over.
    fn save_ui_state(&mut self) {
        self.config.selected_device = self.selected_device.as_ref().map(|id| id.0.clone());
        self.config.selected_exit_node = self.selected_exit_node.as_ref().map(|id| id.0.clone());
        self.config.exit_nodes_page = self.page == Page::ExitNodes;

        let Ok(context) = cosmic_config::Config::new(WINDOW_ID, Config::VERSION) else {
            return;
        };

        if let Err(err) = self.config.write_entry(&context) {
            tracing::warn!(%err, "could not save window state");
        }
    }

    /// Updates the header and window titles.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let window_title = fl!("app-title");

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }
}
