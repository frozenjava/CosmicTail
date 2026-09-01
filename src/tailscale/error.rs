use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    /// Couldn't reach the daemon at all - not running or socket missing
    #[error("tailscaled is not running, or its socket is unreachable")]
    DaemonUnavailable(#[source] io::Error),

    /// HTTP 403. Needs `sudo tailscale set --operator=$USER`
    #[error("tailscaled denied the request: {0}")]
    PermissionDenied(String),

    /// Any other non-success status.
    #[error("tailscaled returned HTTP {status}: {body}")]
    Http { status: u16, body: String },

    /// Connection broke midrequest, or bad HTTP framing
    #[error("connection to tailscaled failed")]
    Transport(#[from] hyper::Error),

    /// Response wasn't the expected JSON
    #[error("could not decode tailscaled's response")]
    Decode(#[from] serde_json::Error),

    /// Malformed request from the client
    #[error("malformed request")]
    BadRequest(#[from] hyper::http::Error),
}

impl Error {
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Error::PermissionDenied(_))
    }

    pub fn is_daemon_unavailable(&self) -> bool {
        matches!(self, Error::DaemonUnavailable(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
