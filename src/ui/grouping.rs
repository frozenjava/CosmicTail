// SPDX-License-Identifier: MIT

//! Grouping the device list by owner, the way the macOS app does.

use crate::fl;
use crate::tailscale::{Device, TailnetStatus};

/// One owner's devices. `devices` keeps the order it had in
/// [`TailnetStatus::devices`], which `sort_for_display` already put
/// online-first, so grouping does not disturb the sort.
pub struct OwnerGroup<'a> {
    pub label: String,
    pub devices: Vec<&'a Device>,
}

/// Group `status`'s peers by owner: this account's devices first, then every
/// other owner alphabetically, then anything tailscaled gave no owner for.
///
/// `include_self` decides whether this machine appears in its owner's group.
/// The window lists it (it is one of your devices); the applet does not,
/// because the root menu already names it on its own line.
pub fn group_by_owner(status: &TailnetStatus, include_self: bool) -> Vec<OwnerGroup<'_>> {
    let my_owner = status.self_device.as_ref().and_then(|d| d.owner.as_deref());

    let mut groups: Vec<(Option<&str>, Vec<&Device>)> = Vec::new();

    // Self leads its own group, matching the window screenshot where this
    // machine sits at the top of "My Devices".
    let self_device = if include_self {
        status.self_device.as_ref()
    } else {
        None
    };

    for device in self_device.into_iter().chain(status.devices.iter()) {
        let owner = device.owner.as_deref();
        match groups.iter_mut().find(|(o, _)| *o == owner) {
            Some((_, list)) => list.push(device),
            None => groups.push((owner, vec![device])),
        }
    }

    groups.sort_by_key(|(owner, _)| match owner {
        // `is_some` matters: when we cannot tell who we are, an unowned group
        // must not silently become "My Devices".
        o if *o == my_owner && o.is_some() => (0, String::new()),
        Some(name) => (1, name.to_lowercase()),
        None => (2, String::new()),
    });

    groups
        .into_iter()
        .map(|(owner, devices)| OwnerGroup {
            label: match owner {
                o if o == my_owner && o.is_some() => fl!("group-my-devices"),
                Some(name) => name.to_owned(),
                None => fl!("group-unowned"),
            },
            devices,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tailscale::Client;

    /// Against the live tailnet: every peer lands in exactly one group, and
    /// this account's devices come first.
    #[tokio::test]
    async fn groups_cover_every_peer() {
        let status = Client::new().status().await.expect("daemon should answer");

        let groups = group_by_owner(&status, false);
        let grouped: usize = groups.iter().map(|g| g.devices.len()).sum();
        assert_eq!(grouped, status.devices.len());

        // Including self adds exactly one device, and no more.
        let with_self = group_by_owner(&status, true);
        let grouped_with_self: usize = with_self.iter().map(|g| g.devices.len()).sum();
        assert_eq!(
            grouped_with_self,
            status.devices.len() + usize::from(status.self_device.is_some())
        );

        // Whoever we are, our own group leads.
        if status
            .self_device
            .as_ref()
            .and_then(|d| d.owner.as_ref())
            .is_some()
        {
            assert_eq!(with_self[0].label, fl!("group-my-devices"));
            assert!(with_self[0].devices.iter().any(|d| d.is_self));
        }
    }

    /// Grouping must not disturb `sort_for_display`, which put online peers
    /// first — that ordering is what the UI relies on inside each group.
    #[tokio::test]
    async fn online_lead_each_group() {
        let status = Client::new().status().await.expect("daemon should answer");

        for group in group_by_owner(&status, false) {
            let mut seen_offline = false;
            for device in group.devices {
                if device.status.is_online() {
                    assert!(
                        !seen_offline,
                        "{} is online but follows an offline device in {}",
                        device.short_name(),
                        group.label
                    );
                } else {
                    seen_offline = true;
                }
            }
        }
    }
}
