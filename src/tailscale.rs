mod client;
mod device;
mod error;
mod model;
pub mod patch;
mod watch;

pub use client::Client;
pub use device::{
    BackendState, ConnPath, Device, DeviceId, DeviceStatus, ExitNodeRole, ExitNodeSuggestion, Os,
    Prefs, TailnetStatus, sort_for_display,
};
pub use error::Error;
pub use patch::PrefsPatch;
pub use watch::{Event, subscription};
