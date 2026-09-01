app-title = Cosmic Tail
app-comment = A TailScale GUI for COSMIC desktop
app-keywords = tailscale;vpn;wireguard;mesh;
applet-title = Cosmic Tail Applet
applet-comment = Tailscale status and controls in the COSMIC panel
about = About
repository = Repository
view = View

## Relative time
ago-days = { $days }d ago
ago-hours = { $hours }h ago
ago-minutes = { $minutes }m ago
ago-just-now = just now

## Backend / bus state
bus-connecting = Connecting...
bus-connected = Connected
bus-disconnected = Disconnected: { $reason }
status-error = Could not read status: { $reason }
tailnet-loading = Contacting tailscaled...
state-running = Connected
state-stopped = Disconnected
state-starting = Connecting...
state-needs-login = Sign in required
state-other = { $state }

## Devices
devices = Devices
device-count = { $online } of { $total } online
group-my-devices = My Devices
group-unowned = Other
device-online = online
device-offline = last seen { $when }
device-offline-unknown = offline
device-expired = key expired
device-expires = expires in { $days }d
device-routes = routes { $routes }
device-exit-active = exit node (active)
device-exit-offered = exit node
this-device = This Device: { $name } ({ $ip })
search-placeholder = Search...
no-devices = No devices match.
select-a-device = Select a device to see its details.

## Device detail
detail-status = Status
detail-ipv4 = IPv4
detail-ipv6 = IPv6
detail-dns = DNS
detail-os = OS
detail-owner = Owner
detail-routes = Routes
detail-expiry = Expiry
detail-expires-in = in { $days }d
detail-expiry-never = never
detail-path = Connection
path-direct = direct ({ $addr })
path-relayed = relayed via { $region }
path-unknown = not connected

## Exit nodes
exit-nodes = Exit Nodes
exit-node = Exit Node
exit-node-none = None
exit-node-recommended = Recommended
exit-node-recommended-label = Recommended Exit Node
exit-node-routing = Routing through this exit node
exit-node-available = Available Exit Nodes
exit-node-use = Use as exit node
# Short form, for the button in the exit-node list where the row already names the device.
exit-node-use-short = Use
exit-node-stop = Stop using
exit-node-none-available = No exit nodes on this tailnet.
run-exit-node = Run Exit Node
run-exit-node-description = Let other devices route their traffic through this machine. Exit nodes must also be approved in the admin console.
allow-lan-access = Allow local network access
allow-lan-access-description = Reach devices on your local network while routing through an exit node.

## Actions
copy-ip = Copy IP
copy-dns = Copy DNS
copy = Copy
copied = Copied
back = Back
connect = Connect
disconnect = Disconnect
network-devices = Network Devices
open = Open
open-window = Open Cosmic Tail
quit = Quit
settings = Settings

## Settings
settings-panel = Panel
panel-applet = Panel applet
panel-applet-description = Show Tailscale status and controls in the COSMIC panel. The panel starts the applet at login and keeps it running.
panel-add = Add to panel
panel-remove = Remove from panel
panel-not-installed = Install Cosmic Tail first. The panel launches applets through their desktop entry, and this one is not on the system yet.
panel-unavailable = Could not read the COSMIC panel configuration.
settings-general = General
settings-accept-dns = Use Tailscale DNS settings
settings-accept-routes = Use Tailscale subnets
settings-accept-routes-description = Route traffic according to your network's rules. Some networks require this to reach addresses that don't start with 100.x.y.z.
settings-allow-incoming = Allow incoming connections
settings-allow-incoming-description = When off, this device can reach others but nothing can reach it.
settings-daemon = Daemon
settings-version = Version
settings-tailnet = Tailnet
settings-operator = Operator
about-license = License
about-disclaimer-affiliation = Cosmic Tail is an unofficial, third-party application. It is not affiliated with, endorsed by, or sponsored by Tailscale Inc. Tailscale is a trademark of Tailscale Inc.
about-disclaimer-warranty = This software is provided as is, without warranty of any kind and without support. Please report problems to this project's issue tracker rather than to Tailscale.

## Errors
write-denied = Write access denied. Run: sudo tailscale set --operator=$USER
write-error = Could not apply change: { $reason }
health-warning = { $warning }
