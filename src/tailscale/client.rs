use std::path::{Path, PathBuf};

use super::device::{self, ExitNodeSuggestion, Prefs, TailnetStatus};
use super::error::{Error, Result};
use crate::tailscale::patch::PrefsPatch;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1::SendRequest;
use hyper::header;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

/// One body type for every request. Reads use `Body::default()` (empty),
/// writes use `Body::from(...)`. Keeping it uniform means one `SendRequest<B>`
/// type instead of a separate one per HTTP method.
pub(super) type Body = Full<Bytes>;

const DEFAULT_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// Every LocalAPI request must carry this `Host` header or tailscaled rejects it.
pub(super) const HOST_HEADER: &str = "local-tailscaled.sock";

/// Holds no connection - each request opens its own. Unix socket connections
/// cost microseconds and it means a dropped connection can never poison the client.
#[derive(Debug, Clone, Hash)]
pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new() -> Self {
        Self {
            socket: PathBuf::from(DEFAULT_SOCKET),
        }
    }

    /// Override the socket path for tests or containers
    pub fn with_socket(path: impl Into<PathBuf>) -> Self {
        Self {
            socket: path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub(super) async fn connect(&self) -> Result<SendRequest<Body>> {
        let stream = UnixStream::connect(&self.socket)
            .await
            .map_err(Error::DaemonUnavailable)?;

        let io = TokioIo::new(stream);
        let (sender, conn) = hyper::client::conn::http1::handshake(io).await?;

        tokio::spawn(async move {
            if let Err(err) = conn.await {
                tracing::debug!(%err, "tailscale connection closed");
            }
        });

        Ok(sender)
    }

    pub(super) async fn request(
        &self,
        method: Method,
        path: &str,
        json_body: Option<Bytes>,
    ) -> Result<Bytes> {
        let mut sender = self.connect().await?;

        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST_HEADER);

        let body = match json_body {
            Some(bytes) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(bytes)
            }
            None => Body::default(),
        };

        let response = sender.send_request(builder.body(body)?).await?;
        let status = response.status();

        let bytes = response.collect().await?.to_bytes();

        if status.is_success() {
            return Ok(bytes);
        }

        let message = String::from_utf8_lossy(&bytes).trim().to_string();
        Err(match status {
            StatusCode::FORBIDDEN => Error::PermissionDenied(message),
            _ => Error::Http {
                status: status.as_u16(),
                body: message,
            },
        })
    }

    pub(super) async fn get(&self, path: &str) -> Result<Bytes> {
        self.request(Method::GET, path, None).await
    }

    pub(super) async fn patch_json(&self, path: &str, body: Bytes) -> Result<Bytes> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn status(&self) -> Result<TailnetStatus> {
        let bytes = self.get("/localapi/v0/status").await?;
        Ok(device::parse_status(&bytes)?)
    }

    pub async fn prefs(&self) -> Result<Prefs> {
        let bytes = self.get("/localapi/v0/prefs").await?;
        Ok(device::parse_prefs(&bytes)?)
    }

    pub async fn set_prefs(&self, patch: PrefsPatch) -> Result<Prefs> {
        let bytes = self
            .patch_json("/localapi/v0/prefs", Bytes::from(patch.into_body()))
            .await?;
        Ok(device::parse_prefs(&bytes)?)
    }

    /// tailscaled's recommended exit node, or `None` if it has no opinion.
    ///
    /// "No suggestion" is not an error condition for the UI, but tailscaled
    /// reports it as a 500 with a prose body, indistinguishable from a genuine
    /// failure without matching on the message. So every HTTP-level answer
    /// becomes `None` and the reason is logged. A dead socket or a rejected
    /// request still propagates, because those mean the whole client is broken
    /// rather than just this one hint.
    pub async fn suggest_exit_node(&self) -> Result<Option<ExitNodeSuggestion>> {
        let bytes = match self.get("/localapi/v0/suggest-exit-node").await {
            Ok(bytes) => bytes,
            Err(err @ Error::Http { .. }) => {
                tracing::debug!(%err, "no exit node suggestion");
                return Ok(None);
            }
            Err(err) => return Err(err),
        };

        match device::parse_exit_node_suggestion(&bytes) {
            Ok(suggestion) => Ok(Some(suggestion)),
            Err(err) => {
                tracing::debug!(%err, "unreadable exit node suggestion");
                Ok(None)
            }
        }
    }

    /// Advertise (or stop advertising) this device as an exit node.
    ///
    /// `AdvertiseRoutes` is replaced wholesale by a write, so this reads the
    /// current list and edits it rather than overwriting it — otherwise
    /// enabling the exit node would silently drop every subnet route the user
    /// is already sharing.
    ///
    /// Read-then-write is not atomic. A concurrent `tailscale set` between the
    /// two calls would be lost; the LocalAPI offers no compare-and-swap, and
    /// the window is microseconds on a unix socket.
    pub async fn set_advertise_exit_node(&self, on: bool) -> Result<Prefs> {
        let current = self.prefs().await?;
        let mut routes: Vec<String> = current.subnet_routes().cloned().collect();

        if on {
            routes.extend(device::EXIT_NODE_ROUTES.iter().map(|r| (*r).to_owned()));
        }

        self.set_prefs(PrefsPatch::new().advertise_routes(routes))
            .await
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connects_to_live_daemon() {
        Client::new()
            .connect()
            .await
            .expect("daemon should be running");
    }

    #[tokio::test]
    async fn missing_socket_is_daemon_unavailable() {
        let e = Client::with_socket("/nope/tailscaled.sock")
            .connect()
            .await
            .unwrap_err();
        assert!(matches!(e, Error::DaemonUnavailable(_)));
    }

    #[tokio::test]
    async fn get_status_succeeds() {
        let b = Client::new().get("/localapi/v0/status").await.unwrap();
        assert!(b.starts_with(b"{"));
    }

    /// Requires `sudo tailscale set --operator=$USER`; without it tailscaled
    /// answers 403 and this fails with [`Error::PermissionDenied`].
    ///
    /// `{}` is a genuine no-op: it names no field, so it changes nothing even
    /// though the write is accepted.
    #[tokio::test]
    async fn empty_write_is_accepted() {
        let body = Client::new()
            .patch_json("/localapi/v0/prefs", Bytes::from_static(b"{}"))
            .await
            .expect("write access; run `sudo tailscale set --operator=$USER`");
        // A successful PATCH echoes back the complete post-write prefs.
        device::parse_prefs(&body).expect("response should be a Prefs object");
    }

    #[tokio::test]
    async fn unknown_path_is_http_error() {
        let e = Client::new().get("/localapi/v0/nope").await.unwrap_err();
        assert!(matches!(e, Error::Http { .. }));
    }

    #[tokio::test]
    async fn status_parses() {
        let s = Client::new().status().await.unwrap();
        assert!(!s.devices.is_empty());
    }

    /// The probed tailnet has an exit node available, so this returns `Some`.
    /// On a tailnet with none it would return `Ok(None)`, not an error.
    #[tokio::test]
    async fn suggest_exit_node_parses() {
        let suggestion = Client::new().suggest_exit_node().await.unwrap();
        if let Some(s) = suggestion {
            assert!(!s.id.0.is_empty());
            // The wire value is an FQDN; `short_name` is the first label.
            assert!(!s.short_name().contains('.'));
        }
    }

    /// Writes the routes already in place, so the daemon ends where it started.
    #[tokio::test]
    async fn advertise_exit_node_round_trips() {
        let client = Client::new();
        let before = client.prefs().await.expect("read prefs");
        let after = client
            .set_advertise_exit_node(before.advertises_exit_node())
            .await
            .expect("write access; run `sudo tailscale set --operator=$USER`");
        assert_eq!(before.advertise_routes, after.advertise_routes);
    }

    #[tokio::test]
    async fn status_propagates_daemon_unavailable() {
        let e = Client::with_socket("/nope/tailscaled.sock")
            .status()
            .await
            .unwrap_err();
        assert!(e.is_daemon_unavailable());
    }
}
