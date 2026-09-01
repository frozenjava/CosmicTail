//! Domain types for the UI.
//!
//! These are what `app.rs` and the `ui` module see. The wire types in
//! [`super::model`] never escape the `tailscale` module.
//!
//! The enums here exist to make tailscaled's awkward sentinel values
//! *unrepresentable* downstream:
//!
//! - `LastSeen` is a zero timestamp (not null, not the Unix epoch) whenever a
//!   peer is online. [`DeviceStatus`] only carries `last_seen` on the variants
//!   where it means something.
//! - `CurAddr` is an empty string when the connection is relayed. [`ConnPath`]
//!   splits that into distinct variants.
//! - `Expired` is `null`-or-`true`, never `false`.

use chrono::{DateTime, Datelike, Duration, Utc};
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use super::model::{ExitNodeSuggestionDto, PeerStatus, PrefsDto, StatusResponse, UserProfile};

/// tailscaled encodes "no timestamp" as Go's zero time, `0001-01-01T00:00:00Z`.
/// That is **not** the Unix epoch (`timestamp() == -62135596800`), so comparing
/// against `UNIX_EPOCH` silently fails. Checking the year is reliable.
pub fn is_zero_time(t: &DateTime<Utc>) -> bool {
    t.year() <= 1
}

fn non_zero(t: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    t.filter(|t| !is_zero_time(t))
}

/// Stable node ID (e.g. `n7a1sm2CNTRL`). Unique across the tailnet, unlike
/// hostnames.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub String);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
    Ios,
    Android,
    FreeBsd,
    Other(String),
}

impl Os {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "linux" => Os::Linux,
            "macos" | "darwin" => Os::MacOs,
            "windows" => Os::Windows,
            "ios" => Os::Ios,
            "android" => Os::Android,
            "freebsd" => Os::FreeBsd,
            _ => Os::Other(s.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Os::Linux => "Linux",
            Os::MacOs => "macOS",
            Os::Windows => "Windows",
            Os::Ios => "iOS",
            Os::Android => "Android",
            Os::FreeBsd => "FreeBSD",
            Os::Other(s) => s,
        }
    }

    /// Freedesktop icon name, for `cosmic::widget::icon::from_name`.
    pub fn icon_name(&self) -> &'static str {
        match self {
            Os::Ios | Os::Android => "phone-symbolic",
            _ => "computer-symbolic",
        }
    }
}

/// Reachability. Ordered tiers drive the device-list sort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    Online,
    Offline {
        last_seen: Option<DateTime<Utc>>,
    },
    /// Node key expired. Implies offline in practice — every expired peer on
    /// the probed tailnet also had `Online: false`.
    Expired {
        last_seen: Option<DateTime<Utc>>,
    },
}

impl DeviceStatus {
    /// Sort tier: online first, then offline, then expired.
    pub fn tier(&self) -> u8 {
        match self {
            DeviceStatus::Online => 0,
            DeviceStatus::Offline { .. } => 1,
            DeviceStatus::Expired { .. } => 2,
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(self, DeviceStatus::Online)
    }

    /// `None` when online (the value is meaningless) or genuinely unknown.
    pub fn last_seen(&self) -> Option<DateTime<Utc>> {
        match self {
            DeviceStatus::Online => None,
            DeviceStatus::Offline { last_seen } | DeviceStatus::Expired { last_seen } => *last_seen,
        }
    }
}

/// How traffic reaches a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnPath {
    /// Direct connection to this endpoint.
    Direct { addr: String },
    /// Relayed through a DERP region (e.g. `mia`).
    Relayed { derp_region: String },
    /// No path established — typical for offline peers.
    Unknown,
}

impl ConnPath {
    pub fn is_direct(&self) -> bool {
        matches!(self, ConnPath::Direct { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitNodeRole {
    /// Cannot act as an exit node.
    NotOffered,
    /// Advertises exit-node capability but is not selected.
    Offered,
    /// Currently the selected exit node.
    Active,
}

impl ExitNodeRole {
    pub fn is_available(&self) -> bool {
        matches!(self, ExitNodeRole::Offered | ExitNodeRole::Active)
    }
}

/// The two CIDRs that mean "I am an exit node" when they appear in
/// `AdvertiseRoutes`. tailscaled advertises both together.
pub const EXIT_NODE_ROUTES: [&str; 2] = ["0.0.0.0/0", "::/0"];

/// True if `route` is one of the exit-node CIDRs rather than a real subnet.
pub fn is_exit_node_route(route: &str) -> bool {
    EXIT_NODE_ROUTES.contains(&route)
}

/// tailscaled's own pick for the best exit node, from
/// `GET /localapi/v0/suggest-exit-node`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitNodeSuggestion {
    pub id: DeviceId,
    /// MagicDNS FQDN, trailing dot stripped.
    pub name: String,
}

impl ExitNodeSuggestion {
    pub(super) fn from_dto(d: ExitNodeSuggestionDto) -> Self {
        ExitNodeSuggestion {
            id: DeviceId(d.id),
            name: d.name.trim_end_matches('.').to_string(),
        }
    }

    /// First DNS label, so it reads the same as a device row's label.
    pub fn short_name(&self) -> &str {
        self.name.split('.').next().unwrap_or(&self.name)
    }
}

/// One machine on the tailnet.
#[derive(Debug, Clone)]
pub struct Device {
    pub id: DeviceId,
    /// `HostName`. Not unique.
    pub name: String,
    /// FQDN, trailing dot stripped.
    pub dns_name: String,
    pub os: Os,
    pub status: DeviceStatus,
    /// Node key expiry. `None` if tailscaled omitted it.
    pub key_expiry: Option<DateTime<Utc>>,
    pub ips: Vec<IpAddr>,
    /// Subnet routes this device is primary for, as CIDR strings for display.
    pub routes: Vec<String>,
    pub exit_node: ExitNodeRole,
    pub path: ConnPath,
    /// Owner's display name, resolved from the status `User` map.
    pub owner: Option<String>,
    pub tags: Vec<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// True for the device this app is running on.
    pub is_self: bool,
}

impl Device {
    pub(super) fn from_peer(
        p: PeerStatus,
        users: &HashMap<String, UserProfile>,
        is_self: bool,
    ) -> Self {
        let last_seen = non_zero(p.last_seen);
        let status = if p.expired.unwrap_or(false) {
            DeviceStatus::Expired { last_seen }
        } else if p.online {
            DeviceStatus::Online
        } else {
            DeviceStatus::Offline { last_seen }
        };

        let path = if !p.cur_addr.is_empty() {
            ConnPath::Direct { addr: p.cur_addr }
        } else if !p.relay.is_empty() {
            ConnPath::Relayed {
                derp_region: p.relay,
            }
        } else {
            ConnPath::Unknown
        };

        let exit_node = if p.exit_node {
            ExitNodeRole::Active
        } else if p.exit_node_option {
            ExitNodeRole::Offered
        } else {
            ExitNodeRole::NotOffered
        };

        let owner = users
            .get(&p.user_id.to_string())
            .map(|u| {
                if u.display_name.is_empty() {
                    u.login_name.clone()
                } else {
                    u.display_name.clone()
                }
            })
            .filter(|s| !s.is_empty());

        Device {
            id: DeviceId(p.id),
            name: p.host_name,
            dns_name: p.dns_name.trim_end_matches('.').to_string(),
            os: Os::parse(&p.os),
            status,
            key_expiry: non_zero(p.key_expiry),
            ips: p.tailscale_ips,
            routes: p.primary_routes,
            exit_node,
            path,
            owner,
            tags: p.tags,
            rx_bytes: p.rx_bytes,
            tx_bytes: p.tx_bytes,
            is_self,
        }
    }

    /// Short label: the MagicDNS leaf, falling back to the hostname.
    pub fn short_name(&self) -> &str {
        self.dns_name
            .split('.')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.name)
    }

    pub fn advertises_routes(&self) -> bool {
        !self.routes.is_empty()
    }

    /// Time until the node key expires. Negative if already past.
    pub fn expires_in(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.key_expiry.map(|e| e - now)
    }

    /// True when the key expires within `days` and has not expired yet.
    pub fn expires_soon(&self, now: DateTime<Utc>, days: i64) -> bool {
        match self.expires_in(now) {
            Some(d) => d > Duration::zero() && d < Duration::days(days),
            None => false,
        }
    }
}

/// Sort for display: online first, then offline by most-recently-seen, then
/// expired. Ties break on name so the order is stable between refreshes.
pub fn sort_for_display(devices: &mut [Device]) {
    devices.sort_by(|a, b| {
        a.status
            .tier()
            .cmp(&b.status.tier())
            // Reversed: most recent first. `None` sorts last within a tier.
            .then_with(|| b.status.last_seen().cmp(&a.status.last_seen()))
            .then_with(|| {
                a.short_name()
                    .to_lowercase()
                    .cmp(&b.short_name().to_lowercase())
            })
    });
}

/// Backend state of the local daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendState {
    NoState,
    NeedsLogin,
    NeedsMachineAuth,
    Stopped,
    Starting,
    Running,
    InUseOtherUser,
    Unknown(String),
}

impl BackendState {
    /// From the string in `/status`. This is the reliable source.
    pub fn parse(s: &str) -> Self {
        match s {
            "NoState" => BackendState::NoState,
            "NeedsLogin" => BackendState::NeedsLogin,
            "NeedsMachineAuth" => BackendState::NeedsMachineAuth,
            "Stopped" => BackendState::Stopped,
            "Starting" => BackendState::Starting,
            "Running" => BackendState::Running,
            "InUseOtherUser" => BackendState::InUseOtherUser,
            other => BackendState::Unknown(other.to_string()),
        }
    }

    /// From the numeric `State` on the watch-ipn-bus. Only `6 == Running` was
    /// confirmed against a live daemon; the rest follow upstream `ipn.State`.
    /// Prefer [`BackendState::parse`] when a `/status` snapshot is available.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => BackendState::NoState,
            1 => BackendState::InUseOtherUser,
            2 => BackendState::NeedsLogin,
            3 => BackendState::NeedsMachineAuth,
            4 => BackendState::Stopped,
            5 => BackendState::Starting,
            6 => BackendState::Running,
            other => BackendState::Unknown(other.to_string()),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, BackendState::Running)
    }
}

/// Local preferences, plus the one thing that decides whether the UI may write.
#[derive(Debug, Clone)]
pub struct Prefs {
    pub want_running: bool,
    pub logged_out: bool,
    /// Selected exit node, if any.
    pub exit_node: Option<DeviceId>,
    pub exit_node_allow_lan_access: bool,
    pub accept_routes: bool,
    pub accept_dns: bool,
    pub shields_up: bool,
    pub advertise_routes: Vec<String>,
    pub run_ssh: bool,
    /// Unix user permitted to write without root. `None` means **root only**,
    /// and every write will return `403 prefs write access denied`.
    pub operator_user: Option<String>,
    pub hostname: String,
}

impl Prefs {
    pub(super) fn from_dto(d: PrefsDto) -> Self {
        Prefs {
            want_running: d.want_running,
            logged_out: d.logged_out,
            exit_node: Some(d.exit_node_id).filter(|s| !s.is_empty()).map(DeviceId),
            exit_node_allow_lan_access: d.exit_node_allow_lan_access,
            accept_routes: d.route_all,
            accept_dns: d.corp_dns,
            shields_up: d.shields_up,
            advertise_routes: d.advertise_routes,
            run_ssh: d.run_ssh,
            operator_user: d.operator_user.filter(|s| !s.is_empty()),
            hostname: d.hostname,
        }
    }

    /// True when this device is advertising itself as an exit node.
    ///
    /// Derived from `AdvertiseRoutes`, not from `/status` — `/status` reports
    /// what *peers* offer, and says nothing about the local node's own
    /// advertisements.
    pub fn advertises_exit_node(&self) -> bool {
        self.advertise_routes.iter().any(|r| is_exit_node_route(r))
    }

    /// Advertised subnet routes with the exit-node CIDRs filtered out, i.e.
    /// the routes a user would recognise as "subnets I share".
    pub fn subnet_routes(&self) -> impl Iterator<Item = &String> {
        self.advertise_routes
            .iter()
            .filter(|r| !is_exit_node_route(r))
    }

    /// Whether `user` may perform writes. When false, show the
    /// `sudo tailscale set --operator=$USER` hint instead of live controls.
    pub fn can_write_as(&self, user: &str) -> bool {
        self.operator_user.as_deref() == Some(user)
    }
}

/// Everything the UI needs for one render, built from a `/status` snapshot.
#[derive(Debug, Clone)]
pub struct TailnetStatus {
    pub backend: BackendState,
    pub daemon_version: String,
    pub tailnet_name: Option<String>,
    pub magic_dns_suffix: String,
    /// This device's own addresses.
    pub ips: Vec<IpAddr>,
    /// Human-readable warnings from tailscaled. Empty when healthy.
    pub health: Vec<String>,
    /// URL to visit when `backend` is `NeedsLogin`.
    pub auth_url: Option<String>,
    pub self_device: Option<Device>,
    /// Peers, already sorted by [`sort_for_display`].
    pub devices: Vec<Device>,
    /// The active exit node, if one is selected.
    pub exit_node: Option<DeviceId>,
}

impl TailnetStatus {
    pub(super) fn from_status(s: StatusResponse) -> Self {
        let users = s.user;

        let mut devices: Vec<Device> = s
            .peer
            .into_values()
            .map(|p| Device::from_peer(p, &users, false))
            .collect();
        sort_for_display(&mut devices);

        let exit_node = devices
            .iter()
            .find(|d| d.exit_node == ExitNodeRole::Active)
            .map(|d| d.id.clone());

        TailnetStatus {
            backend: BackendState::parse(&s.backend_state),
            daemon_version: s.version,
            tailnet_name: s.current_tailnet.map(|t| t.name).filter(|n| !n.is_empty()),
            magic_dns_suffix: s.magic_dns_suffix,
            ips: s.tailscale_ips,
            health: s.health,
            auth_url: Some(s.auth_url).filter(|u| !u.is_empty()),
            self_device: s.self_node.map(|p| Device::from_peer(p, &users, true)),
            devices,
            exit_node,
        }
    }

    /// Find a device by id, across the peers *and* this machine.
    pub fn device(&self, id: &DeviceId) -> Option<&Device> {
        self.self_device
            .iter()
            .chain(self.devices.iter())
            .find(|d| &d.id == id)
    }

    /// The device traffic is currently being routed through, if any.
    pub fn active_exit_node(&self) -> Option<&Device> {
        self.exit_node.as_ref().and_then(|id| self.device(id))
    }

    /// Peers that can currently serve as an exit node, for a picker.
    ///
    /// Expired nodes are excluded — they advertise the capability but cannot
    /// carry traffic. Offline ones are kept so the UI can show them greyed out
    /// rather than having them vanish from the list.
    pub fn exit_node_candidates(&self) -> impl Iterator<Item = &Device> {
        self.devices
            .iter()
            .filter(|d| d.exit_node.is_available())
            .filter(|d| !matches!(d.status, DeviceStatus::Expired { .. }))
    }

    pub fn online_count(&self) -> usize {
        self.devices.iter().filter(|d| d.status.is_online()).count()
    }
}

// ---------------------------------------------------------------------------
// Parse entry points
//
// These are the only doors between the wire types and the rest of the app.
// Keeping them here means `model` never has to be made public.
// ---------------------------------------------------------------------------

/// Parse a `GET /localapi/v0/status` body into a display-ready snapshot.
pub fn parse_status(bytes: &[u8]) -> serde_json::Result<TailnetStatus> {
    Ok(TailnetStatus::from_status(serde_json::from_slice(bytes)?))
}

/// Parse a `GET /localapi/v0/prefs` body.
pub fn parse_prefs(bytes: &[u8]) -> serde_json::Result<Prefs> {
    Ok(Prefs::from_dto(serde_json::from_slice(bytes)?))
}

/// Parse a `GET /localapi/v0/suggest-exit-node` body.
pub fn parse_exit_node_suggestion(bytes: &[u8]) -> serde_json::Result<ExitNodeSuggestion> {
    Ok(ExitNodeSuggestion::from_dto(serde_json::from_slice(bytes)?))
}

/// Parse one newline-delimited object from `GET /localapi/v0/watch-ipn-bus`.
///
/// Pass a single line with the trailing `\n` already stripped.
pub fn parse_notify(line: &[u8]) -> serde_json::Result<BusEvent> {
    let n: super::model::Notify = serde_json::from_slice(line)?;
    Ok(BusEvent {
        state: n.state.map(BackendState::from_code),
        prefs: n.prefs.map(Prefs::from_dto),
        netmap_changed: n.net_map.is_some(),
        error: n.err_message,
        browse_to_url: n.browse_to_url,
    })
}

/// A decoded bus notification. Every field is independently optional — a given
/// line reports only what changed.
#[derive(Debug, Clone)]
pub struct BusEvent {
    /// Backend state transition (connected, needs login, stopped, ...).
    pub state: Option<BackendState>,
    /// Preferences changed — including the exit node selection.
    pub prefs: Option<Prefs>,
    /// The network map changed: peers appeared, went offline, or altered
    /// routes. The payload is discarded; re-fetch `/status` and rebuild the
    /// device list, which is cheap and avoids duplicating the netmap schema.
    pub netmap_changed: bool,
    /// Backend error message.
    pub error: Option<String>,
    /// Login URL to open, when the daemon needs interactive auth.
    pub browse_to_url: Option<String>,
}

impl BusEvent {
    /// True when tailscaled sent a notification carrying nothing this app models.
    /// Roughly half of live pushes are empty; folding them into state would
    /// cause pointless re-renders
    pub fn is_empty(&self) -> bool {
        self.state.is_none()
            && self.prefs.is_none()
            && !self.netmap_changed
            && self.error.is_none()
            && self.browse_to_url.is_none()
    }
}
