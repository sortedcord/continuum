//! Runs an established [`Session`] in the background: keepalives, Ping
//! replies, and channels for everything else.
//!
//! The stream is split so that reading a frame is never cancelled halfway:
//! one task only reads frames, and the driver selects over channels, which
//! is cancel-safe.

use std::time::Duration;

use continuum_core::proto::v1::{Envelope, Goodbye, Hello, Ping, Pong, envelope};
use continuum_core::trust::TrustedPeer;
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, interval_at, sleep_until};

use crate::NetError;
use crate::frame::{read_frame, write_frame};
use crate::session::Session;

/// Why a link ended.
#[derive(Debug)]
pub enum LinkClosed {
    /// The peer said goodbye.
    Goodbye(i32),
    /// We closed it ([`SessionLink::close`] or dropped the sender).
    Local,
    /// Nothing heard within the idle timeout.
    TimedOut,
    Error(NetError),
}

/// Handle to a running session.
#[derive(Debug)]
pub struct SessionLink {
    pub peer: TrustedPeer,
    pub hello: Hello,
    /// Messages from the peer, except Ping/Pong which are handled here.
    pub incoming: mpsc::Receiver<envelope::Body>,
    outgoing: mpsc::Sender<envelope::Body>,
    /// Resolves when the link ends.
    pub closed: oneshot::Receiver<LinkClosed>,
}

impl SessionLink {
    /// # Errors
    /// Fails if the link has already closed.
    pub async fn send(&self, body: envelope::Body) -> Result<(), NetError> {
        self.outgoing
            .send(body)
            .await
            .map_err(|_| NetError::Protocol("link closed"))
    }

    /// A sender that can be cloned into other tasks. The link stays open
    /// while any sender (including this handle) is alive.
    pub fn sender(&self) -> mpsc::Sender<envelope::Body> {
        self.outgoing.clone()
    }

    /// Splits off the receiving halves, dropping this handle's own sender.
    pub fn into_receivers(
        self,
    ) -> (
        TrustedPeer,
        Hello,
        mpsc::Receiver<envelope::Body>,
        oneshot::Receiver<LinkClosed>,
    ) {
        (self.peer, self.hello, self.incoming, self.closed)
    }
}

/// Starts driving `session`. A Ping is sent every `keepalive`; the link
/// closes if nothing arrives for three keepalive periods.
pub fn spawn_link<S>(session: Session<S>, keepalive: Duration) -> SessionLink
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (peer, hello, stream) = session.into_parts();
    let (stream, channel) = stream.into_inner();
    let (mut reader, writer) = tokio::io::split(stream);

    let (frames_tx, frames_rx) = mpsc::channel::<Result<Vec<u8>, NetError>>(32);
    let reader_task = tokio::spawn(async move {
        loop {
            let frame = read_frame(&mut reader).await;
            let failed = frame.is_err();
            if frames_tx.send(frame).await.is_err() || failed {
                break;
            }
        }
    });

    let (incoming_tx, incoming) = mpsc::channel(32);
    let (outgoing, outgoing_rx) = mpsc::channel(32);
    let (closed_tx, closed) = oneshot::channel();
    tokio::spawn(async move {
        let reason = drive(
            channel,
            writer,
            frames_rx,
            incoming_tx,
            outgoing_rx,
            keepalive,
        )
        .await;
        reader_task.abort();
        let _ = closed_tx.send(reason);
    });

    SessionLink {
        peer,
        hello,
        incoming,
        outgoing,
        closed,
    }
}

async fn drive<W: AsyncWrite + Unpin>(
    mut channel: continuum_core::session::SecureChannel,
    mut writer: W,
    mut frames: mpsc::Receiver<Result<Vec<u8>, NetError>>,
    incoming: mpsc::Sender<envelope::Body>,
    mut outgoing: mpsc::Receiver<envelope::Body>,
    keepalive: Duration,
) -> LinkClosed {
    let send = async |channel: &mut continuum_core::session::SecureChannel,
                      writer: &mut W,
                      body: envelope::Body|
           -> Result<(), NetError> {
        let bytes = Envelope { body: Some(body) }.encode_to_vec();
        write_frame(writer, &channel.encrypt(&bytes)?).await
    };

    let mut ticks = interval_at(Instant::now() + keepalive, keepalive);
    let idle = keepalive * 3;
    let mut deadline = Instant::now() + idle;
    let mut nonce = 0u64;
    loop {
        let result = tokio::select! {
            frame = frames.recv() => {
                let frame = match frame {
                    Some(Ok(frame)) => frame,
                    Some(Err(err)) => return LinkClosed::Error(err),
                    None => return LinkClosed::Error(NetError::Protocol("reader stopped")),
                };
                deadline = Instant::now() + idle;
                let envelope = match channel
                    .decrypt(&frame)
                    .map_err(NetError::from)
                    .and_then(|plain| Envelope::decode(plain.as_slice()).map_err(NetError::from))
                {
                    Ok(envelope) => envelope,
                    Err(err) => return LinkClosed::Error(err),
                };
                match envelope.body {
                    Some(envelope::Body::Ping(Ping { nonce })) => {
                        send(&mut channel, &mut writer, envelope::Body::Pong(Pong { nonce })).await
                    }
                    Some(envelope::Body::Pong(_)) => Ok(()),
                    Some(envelope::Body::Goodbye(goodbye)) => return LinkClosed::Goodbye(goodbye.reason),
                    Some(body) => {
                        // A slow consumer only delays this link, never others.
                        let _ = incoming.send(body).await;
                        Ok(())
                    }
                    None => Err(NetError::Protocol("empty envelope")),
                }
            }
            body = outgoing.recv() => {
                let Some(body) = body else {
                    let goodbye = Goodbye { reason: continuum_core::proto::v1::goodbye::Reason::Shutdown.into() };
                    let _ = send(&mut channel, &mut writer, envelope::Body::Goodbye(goodbye)).await;
                    return LinkClosed::Local;
                };
                send(&mut channel, &mut writer, body).await
            }
            _ = ticks.tick() => {
                nonce += 1;
                send(&mut channel, &mut writer, envelope::Body::Ping(Ping { nonce })).await
            }
            () = sleep_until(deadline) => return LinkClosed::TimedOut,
        };
        if let Err(err) = result {
            return LinkClosed::Error(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use continuum_core::clock::Timestamp;
    use continuum_core::entropy::DeterministicEntropy;
    use continuum_core::identity::DeviceIdentity;
    use continuum_core::proto::v1::{DeviceInfo, Hello};

    use super::*;
    use crate::frame::read_preamble;
    use crate::session::{accept_session, connect_session};

    fn identity(seed: u64) -> DeviceIdentity {
        DeviceIdentity::generate(&mut DeterministicEntropy::new(seed))
    }

    fn trusted(identity: &DeviceIdentity) -> TrustedPeer {
        TrustedPeer {
            identity: identity.public(),
            presence_key: identity.presence_key().clone(),
            name: String::new(),
            paired_at: Timestamp::default(),
        }
    }

    fn hello() -> Hello {
        Hello {
            device: Some(DeviceInfo::default()),
            manifest: None,
            presence_key: Vec::new(),
        }
    }

    async fn linked(keepalive: Duration) -> (SessionLink, SessionLink) {
        let (laptop, phone) = (identity(1), identity(2));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let (phone_peer, laptop_peer) = (trusted(&phone), trusted(&laptop));
        let client = connect_session(a, &laptop, &phone_peer, hello());
        let server = async {
            read_preamble(&mut b).await?;
            accept_session(b, &phone, &[laptop_peer], hello()).await
        };
        let (client, server) = tokio::join!(client, server);
        (
            spawn_link(client.unwrap(), keepalive),
            spawn_link(server.unwrap(), keepalive),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn keepalives_keep_link_up() {
        let (a, mut b) = linked(Duration::from_secs(10)).await;
        tokio::time::sleep(Duration::from_secs(120)).await;
        a.send(envelope::Body::Hello(hello())).await.unwrap();
        assert!(matches!(
            b.incoming.recv().await,
            Some(envelope::Body::Hello(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_sender_says_goodbye() {
        let (a, b) = linked(Duration::from_secs(10)).await;
        let SessionLink {
            outgoing, closed, ..
        } = a;
        drop(outgoing);
        assert!(matches!(closed.await.unwrap(), LinkClosed::Local));
        assert!(matches!(b.closed.await.unwrap(), LinkClosed::Goodbye(_)));
    }

    #[tokio::test(start_paused = true)]
    async fn silent_peer_times_out() {
        let (laptop, phone) = (identity(1), identity(2));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let (phone_peer, laptop_peer) = (trusted(&phone), trusted(&laptop));
        let client = connect_session(a, &laptop, &phone_peer, hello());
        let server = async {
            read_preamble(&mut b).await?;
            accept_session(b, &phone, &[laptop_peer], hello()).await
        };
        let (client, server) = tokio::join!(client, server);
        let link = spawn_link(client.unwrap(), Duration::from_secs(10));
        // Hold the server session open but never read or answer.
        let _silent = server.unwrap();
        assert!(matches!(link.closed.await.unwrap(), LinkClosed::TimedOut));
    }
}
