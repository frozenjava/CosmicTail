use super::device::DeviceId;
use serde_json::{Map, Value};

/// tailscaled's `PATCH /localapi/v0/prefs` takes Go's `ipn.MaskedPrefs`: every
/// field travels with a `<Field>Set: true` flag. Both halves are load-bearing —
/// a value without its flag is ignored, and a flag without its value writes
/// Go's zero value. Neither mistake produces an error. This builder always
/// writes the pair, so neither is representable.
#[derive(Debug, Clone, Default)]
pub struct PrefsPatch(Map<String, Value>);

impl PrefsPatch {
    pub fn new() -> Self {
        Self::default()
    }

    fn set(mut self, field: &str, value: impl Into<Value>) -> Self {
        self.0.insert(field.to_owned(), value.into());
        self.0.insert(format!("{field}Set"), Value::Bool(true));
        self
    }

    pub fn want_running(self, on: bool) -> Self {
        self.set("WantRunning", on)
    }

    /// `None` clears the exit node. Uses `ExitNodeID`, never `ExitNodeIP`:
    /// tailscaled accepts both fields without complaint, so mixing them is a
    /// silent footgun rather than an error.
    pub fn exit_node(self, id: Option<&DeviceId>) -> Self {
        self.set("ExitNodeID", id.map_or("", |d| d.0.as_str()))
    }

    pub fn exit_node_allow_lan_access(self, on: bool) -> Self {
        self.set("ExitNodeAllowLANAccess", on)
    }

    pub fn accept_routes(self, on: bool) -> Self {
        self.set("RouteAll", on)
    }

    pub fn accept_dns(self, on: bool) -> Self {
        self.set("CorpDNS", on)
    }

    pub fn shields_up(self, on: bool) -> Self {
        self.set("ShieldsUp", on)
    }

    pub fn advertise_routes(self, routes: Vec<String>) -> Self {
        self.set("AdvertiseRoutes", routes)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn into_body(self) -> Vec<u8> {
        serde_json::to_vec(&Value::Object(self.0)).expect("prefs patch is always valid JSON")
    }
}
