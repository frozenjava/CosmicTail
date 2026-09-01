//! Wire types for the tailscaled LocalAPI.
//!
//! These mirror tailscaled's JSON **exactly** and are visible only inside the
//! `tailscale` module (`pub(super)`). Nothing outside it should ever see one —
//! convert to the domain types in [`super::device`] at the boundary.
//!
//! Two things about this JSON drive the annotations below:
//!
//! 1. Go's `omitempty` means a field can be **absent** entirely. Verified on a
//!    live tailnet: `Expired` was present on 8 of 22 peers, `PrimaryRoutes` on 2.
//! 2. Fields can also be explicitly `null` (e.g. `"Health": null`).
//!
//! `#[serde(default)]` covers case 1 only. [`null_as_default`] covers both, so
//! collections use `#[serde(default, deserialize_with = "null_as_default")]`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::net::IpAddr;

/// Accepts a missing key, an explicit `null`, or a real value.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `GET /localapi/v0/status`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct StatusResponse {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub backend_state: String,
    #[serde(rename = "AuthURL", default)]
    pub auth_url: String,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
    pub tailscale_ips: Vec<IpAddr>,
    /// `Self` is a Rust keyword, hence the rename.
    #[serde(rename = "Self")]
    pub self_node: Option<PeerStatus>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub health: Vec<String>,
    #[serde(rename = "MagicDNSSuffix", default)]
    pub magic_dns_suffix: String,
    pub current_tailnet: Option<CurrentTailnet>,
    /// Keyed by `nodekey:<hex>`. The key is *not* the stable node ID — that is
    /// the peer's own `ID` field.
    #[serde(default, deserialize_with = "null_as_default")]
    pub peer: HashMap<String, PeerStatus>,
    /// Keyed by the stringified numeric user ID.
    #[serde(default, deserialize_with = "null_as_default")]
    pub user: HashMap<String, UserProfile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct CurrentTailnet {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "MagicDNSSuffix", default)]
    pub magic_dns_suffix: String,
    #[serde(rename = "MagicDNSEnabled", default)]
    pub magic_dns_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct UserProfile {
    #[serde(rename = "ID", default)]
    pub id: i64,
    #[serde(default)]
    pub login_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(rename = "ProfilePicURL", default)]
    pub profile_pic_url: String,
}

/// One entry of the `Peer` map, and also the shape of `Self`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct PeerStatus {
    /// Stable node ID, e.g. `n7a1sm2CNTRL`. Unique; use as identity.
    #[serde(rename = "ID", default)]
    pub id: String,
    #[serde(rename = "NodeID", default)]
    pub node_id: i64,
    #[serde(default)]
    pub public_key: String,
    /// Not unique — three peers on the probed tailnet were all `localhost`.
    #[serde(default)]
    pub host_name: String,
    /// FQDN with a trailing dot, e.g. `framework.tailbab6c.ts.net.`
    #[serde(rename = "DNSName", default)]
    pub dns_name: String,
    #[serde(rename = "OS", default)]
    pub os: String,
    #[serde(rename = "UserID", default)]
    pub user_id: i64,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
    pub tailscale_ips: Vec<IpAddr>,
    /// Superset: node IPs + approved subnet routes + `0.0.0.0/0`,`::/0` when
    /// exit-node capable. Prefer `primary_routes` / `exit_node_option`.
    #[serde(rename = "AllowedIPs", default, deserialize_with = "null_as_default")]
    pub allowed_ips: Vec<String>,
    /// Subnet routes this peer is actively primary for. Often absent.
    #[serde(default, deserialize_with = "null_as_default")]
    pub primary_routes: Vec<String>,
    /// Direct endpoint. **Empty string means the connection is relayed.**
    #[serde(default)]
    pub cur_addr: String,
    /// DERP region code, e.g. `mia`.
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub online: bool,
    /// **Zero-valued (`0001-01-01T00:00:00Z`) whenever `online` is true.**
    /// Never render directly; see `device::is_zero_time`.
    pub last_seen: Option<DateTime<Utc>>,
    pub key_expiry: Option<DateTime<Utc>>,
    /// `null`/absent when the key is fine, `true` when expired. Never `false`.
    #[serde(default)]
    pub expired: Option<bool>,
    /// This peer *can* act as an exit node.
    #[serde(default)]
    pub exit_node_option: bool,
    /// This peer *is* the currently selected exit node.
    #[serde(default)]
    pub exit_node: bool,
    #[serde(default)]
    pub rx_bytes: u64,
    #[serde(default)]
    pub tx_bytes: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
    pub created: Option<DateTime<Utc>>,
}

/// `GET`/`PATCH /localapi/v0/prefs`
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct PrefsDto {
    #[serde(default)]
    pub want_running: bool,
    #[serde(default)]
    pub logged_out: bool,
    #[serde(rename = "ExitNodeID", default)]
    pub exit_node_id: String,
    #[serde(rename = "ExitNodeIP", default)]
    pub exit_node_ip: String,
    #[serde(default)]
    pub exit_node_allow_lan_access: bool,
    #[serde(default)]
    pub route_all: bool,
    #[serde(rename = "CorpDNS", default)]
    pub corp_dns: bool,
    #[serde(default)]
    pub shields_up: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    pub advertise_routes: Vec<String>,
    #[serde(rename = "RunSSH", default)]
    pub run_ssh: bool,
    /// `null` on a fresh install — meaning **only root may write**.
    #[serde(default)]
    pub operator_user: Option<String>,
    #[serde(default)]
    pub hostname: String,
}

/// One newline-delimited object from `GET /localapi/v0/watch-ipn-bus`.
/// Every payload field is nullable; only what changed is non-null.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Notify {
    #[serde(default)]
    pub version: String,
    #[serde(rename = "SessionID", default)]
    pub session_id: Option<String>,
    pub err_message: Option<String>,
    /// Numeric `ipn.State`. Only `6 == Running` was confirmed on a live daemon.
    pub state: Option<i32>,
    pub prefs: Option<PrefsDto>,
    /// Presence-only: this empty struct deserializes from any object and
    /// discards the contents. The netmap's peer shape differs from `/status`
    /// (peers carry `Name`, not `HostName`), so the cheap, reliable move is to
    /// treat this as a "something changed" signal and re-fetch `/status`.
    pub net_map: Option<NetMapChanged>,
    #[serde(rename = "BrowseToURL")]
    pub browse_to_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct NetMapChanged {}

/// `GET /localapi/v0/suggest-exit-node`
///
/// Verified live: `{"ID":"njqFxtx9kp11CNTRL","Name":"homeassistant.tailbab6c.ts.net."}`.
/// `Location` also appears for Mullvad nodes; we do not model it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ExitNodeSuggestionDto {
    #[serde(rename = "ID", default)]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
}
