use std::time::Duration;

use cosmic::iced::Subscription;
use cosmic::iced::futures::SinkExt;
use cosmic::iced::futures::channel::mpsc::Sender;
use cosmic::iced::stream;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Request, StatusCode, header};

use super::client::{Body, Client, HOST_HEADER};
use super::device::{self, BusEvent};
use super::error::{Error, Result};

pub fn subscription(client: Client) -> Subscription<Event> {
    Subscription::run_with(client, |client| {
        let client = client.clone();
        stream::channel(16, |sink| run(client, sink))
    })
}

/// Initial state + initial prefs + initial netmap, private keys omitted.
/// Bit 1 (engine status) is deliberately excluded: it fires ~1×/sec whether or
/// not anything changed.
pub(super) const MASK_DEFAULT: u32 = 2 | 4 | 8 | 16;

/// What the UI receives from the bus.
///
/// `Error` is not `Clone` (it wraps `io::Error`), but iced messages must be
/// so failures cross this boundary as already-formatted text
#[derive(Debug, Clone)]
pub enum Event {
    /// The bus is live. Re-fetch `/status`: anything missed while
    /// disconnected is invisible here.
    Connected,
    Notify(BusEvent),
    Disconnected(String),
}

pub(super) struct BusStream {
    body: Incoming,
    buf: Vec<u8>,
}

fn take_line(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let newline = buf.iter().position(|b| *b == b'\n')?;
    let mut line: Vec<u8> = buf.drain(..=newline).collect();
    line.pop();
    Some(line)
}

impl BusStream {
    pub(super) fn new(body: Incoming) -> Self {
        Self {
            body,
            buf: Vec::new(),
        }
    }

    /// The next notification
    ///
    /// `None` means tailscaled closed the stream - reconnect.
    pub(super) async fn next_event(&mut self) -> Option<Result<BusEvent>> {
        loop {
            // 1 emit from the buffer first: one chunk can hold several lines.
            if let Some(line) = take_line(&mut self.buf) {
                if line.is_empty() {
                    continue;
                }
                return Some(device::parse_notify(&line).map_err(Error::from));
            }
            // 2. no complete line buffered - pull another chunk and retry
            match self.body.frame().await {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        self.buf.extend_from_slice(data);
                    }
                }
                Some(Err(e)) => return Some(Err(Error::Transport(e))),
                None => return None,
            }
        }
    }
}

pub(super) async fn open_bus(client: &Client, mask: u32) -> Result<Incoming> {
    let mut sender = client.connect().await?;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/localapi/v0/watch-ipn-bus?mask={mask}"))
        .header(header::HOST, HOST_HEADER)
        .body(Body::default())?;

    let response = sender.send_request(request).await?;
    let status = response.status();

    if status.is_success() {
        return Ok(response.into_body());
    }

    let bytes = response.collect().await?.to_bytes();
    let message = String::from_utf8_lossy(&bytes).trim().to_owned();
    Err(match status {
        StatusCode::FORBIDDEN => Error::PermissionDenied(message),
        _ => Error::Http {
            status: status.as_u16(),
            body: message,
        },
    })
}

/// Wait before the next reconnect: 1s, 2, 4, 8, 16, then 30s forever.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs(2u64.pow(failures.min(5)).min(30))
}

/// Own the bus for the lifetime of the app, reconnecting as needed.
///
/// Never returns on its own while the bus is idle - it parks inside
/// [`BusStream::next_event`]. iced ends a subscription by dropping its future,
/// which cancels that await. Do not `tokio::spawn` this.
pub(super) async fn run(client: Client, mut sink: Sender<Event>) {
    let mut failures = 0u32;

    loop {
        match open_bus(&client, MASK_DEFAULT).await {
            Ok(body) => {
                if sink.send(Event::Connected).await.is_err() {
                    return;
                }

                let mut bus = BusStream::new(body);
                let mut delivered = false;

                while let Some(next) = bus.next_event().await {
                    match next {
                        // A malformed line is not worth dropping the stream over.
                        Err(err) => tracing::warn!(%err, "unparsable bus line"),
                        Ok(event) if event.is_empty() => {}
                        Ok(event) => {
                            delivered = true;
                            if sink.send(Event::Notify(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                }

                // `next_event` returned None: tailscaled closed the stream.
                // Only a connection that produced something counts as healthy;
                // otherwise an instantly-closing bus would spin at 1s forever.
                failures = if delivered { 0 } else { failures + 1 };
                if sink
                    .send(Event::Disconnected(
                        "tailscaled closed the connection".to_owned(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(err) => {
                failures += 1;
                if sink
                    .send(Event::Disconnected(err.to_string()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }

        tokio::time::sleep(backoff(failures)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::patch::PrefsPatch;
    use super::*;
    use cosmic::iced::futures::{self, StreamExt};

    fn v(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[tokio::test]
    async fn opens_and_streams() {
        let mut body = open_bus(&Client::new(), 30).await.expect("bus should open");
        let frame = tokio::time::timeout(Duration::from_secs(3), body.frame())
            .await
            .expect("data should arrive")
            .expect("stream should not end")
            .expect("frame should not error");
        assert!(frame.data_ref().is_some_and(|d| !d.is_empty()));
    }

    #[test]
    fn take_line_leaves_partial_lines_buffered() {
        let mut b = v("{\"a\":1}\n{\"par");
        assert_eq!(take_line(&mut b).unwrap(), v("{\"a\":1}"));
        assert_eq!(take_line(&mut b), None);
        assert_eq!(b, v("{\"par")); // kept for the next chunk
        b.extend_from_slice(b"tial\":2}\n");
        assert_eq!(take_line(&mut b).unwrap(), v("{\"partial\":2}"));
    }

    #[test]
    fn backoff_grows_then_caps() {
        let secs: Vec<u64> = (0..8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[tokio::test]
    async fn delivers_connected_then_live_changes() {
        let (tx, mut rx) = futures::channel::mpsc::channel(16);
        let mut handle = tokio::spawn(run(Client::new(), tx));

        assert!(matches!(rx.next().await, Some(Event::Connected)));

        // Initial burst: state + prefs + netmap in one notification.
        let first = rx.next().await.expect("initial notify");
        assert!(matches!(&first, Event::Notify(e) if e.state.is_some()));

        // An external change must arrive as a push.
        let client = Client::new();
        client
            .set_prefs(PrefsPatch::new().exit_node(Some(&device::DeviceId("nSEED".to_owned()))))
            .await
            .unwrap();
        let pushed = tokio::time::timeout(Duration::from_secs(3), rx.next())
            .await
            .expect("a live change should push within 3s")
            .expect("stream should not end");
        assert!(matches!(&pushed, Event::Notify(e) if e.prefs.is_some()));

        client
            .set_prefs(PrefsPatch::new().exit_node(None))
            .await
            .unwrap();
        assert_eq!(
            client.prefs().await.unwrap().exit_node,
            None,
            "cleanup failed"
        );

        // Dropping the receiver does NOT end `run`: it is parked in
        // `next_event()` on an idle body and never reaches a `send`.
        drop(rx);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), &mut handle)
                .await
                .is_err(),
            "run unexpectedly returned on its own"
        );

        // iced cancels by dropping the future; `abort` is the tokio equivalent.
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
    }
}
