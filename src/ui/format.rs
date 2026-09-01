// SPDX-License-Identifier: MIT

//! Turning domain values into the strings both UIs print.

use chrono::{DateTime, Utc};

use crate::fl;
use crate::tailscale::{Device, DeviceStatus, ExitNodeRole};

/// Human-readable "time since", coarsest useful unit only.
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let elapsed = now - then;
    if elapsed.num_days() > 0 {
        fl!("ago-days", days = elapsed.num_days())
    } else if elapsed.num_hours() > 0 {
        fl!("ago-hours", hours = elapsed.num_hours())
    } else if elapsed.num_minutes() > 0 {
        fl!("ago-minutes", minutes = elapsed.num_minutes())
    } else {
        fl!("ago-just-now")
    }
}

/// Reachability on its own: "online", "last seen 3d ago", "key expired".
pub fn status_label(device: &Device, now: DateTime<Utc>) -> String {
    match &device.status {
        DeviceStatus::Online => fl!("device-online"),
        DeviceStatus::Expired { .. } => fl!("device-expired"),
        DeviceStatus::Offline { last_seen } => match last_seen {
            Some(seen) => fl!("device-offline", when = ago(*seen, now)),
            None => fl!("device-offline-unknown"),
        },
    }
}

/// The parenthesised suffix the macOS menu puts after a name, e.g.
/// `joshuas-mac-mini (offline)`. Empty when the device is online, because the
/// common case should not carry a qualifier.
pub fn name_suffix(device: &Device) -> String {
    match device.status {
        DeviceStatus::Online => String::new(),
        DeviceStatus::Expired { .. } => format!(" ({})", fl!("device-expired")),
        DeviceStatus::Offline { .. } => format!(" ({})", fl!("device-offline-unknown")),
    }
}

/// One line describing reachability, expiry, routes and exit-node role.
pub fn summary(device: &Device, now: DateTime<Utc>) -> String {
    let mut parts = vec![status_label(device, now)];

    // Only worth surfacing while it is still actionable.
    if device.expires_soon(now, 30)
        && let Some(remaining) = device.expires_in(now)
    {
        parts.push(fl!("device-expires", days = remaining.num_days()));
    }

    if device.advertises_routes() {
        parts.push(fl!("device-routes", routes = device.routes.join(", ")));
    }

    match device.exit_node {
        ExitNodeRole::Active => parts.push(fl!("device-exit-active")),
        ExitNodeRole::Offered => parts.push(fl!("device-exit-offered")),
        ExitNodeRole::NotOffered => {}
    }

    parts.join(" \u{b7} ")
}

/// The device's primary address, as displayed. Empty if it somehow has none.
pub fn primary_ip(device: &Device) -> String {
    device
        .ips
        .first()
        .map(ToString::to_string)
        .unwrap_or_default()
}
