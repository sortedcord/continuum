use continuum_core::identity::DeviceIdentity;
use continuum_core::proto::v1::{Envelope, Hello, envelope};
use continuum_core::session::{Handshake, HandshakeKind, Role, SecureChannel};
use continuum_core::trust::TrustedPeer;
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::NetError;
use crate::frame::{Intent, read_frame, write_frame, write_preamble};

/// A byte stream wrapped in a [`SecureChannel`]; one frame per message.
#[derive(Debug)]
pub struct SecureStream<S> {
    stream: S,
    channel: SecureChannel,
}

impl<S: AsyncRead + AsyncWrite + Unpin> SecureStream<S> {
    pub(crate) fn new(stream: S, channel: SecureChannel) -> Self {
        Self { stream, channel }
    }

    pub fn channel(&self) -> &SecureChannel {
        &self.channel
    }

    pub fn into_inner(self) -> (S, SecureChannel) {
        (self.stream, self.channel)
    }

    /// # Errors
    /// Fails on I/O or encryption errors.
    pub async fn send<M: Message>(&mut self, message: &M) -> Result<(), NetError> {
        let ciphertext = self.channel.encrypt(&message.encode_to_vec())?;
        write_frame(&mut self.stream, &ciphertext).await
    }

    /// # Errors
    /// Fails on I/O errors, tampering, or undecodable messages.
    pub async fn recv<M: Message + Default>(&mut self) -> Result<M, NetError> {
        let frame = read_frame(&mut self.stream).await?;
        let plaintext = self.channel.decrypt(&frame)?;
        Ok(M::decode(plaintext.as_slice())?)
    }
}

/// Runs handshake messages until done. `check` sees the remote static key
/// as soon as it is known and can abort before we reply.
pub(crate) async fn drive<S, F>(
    stream: &mut S,
    mut handshake: Handshake,
    mut check: F,
) -> Result<SecureChannel, NetError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnMut(&Handshake) -> Result<(), NetError>,
{
    while !handshake.is_finished() {
        if handshake.is_my_turn() {
            write_frame(stream, &handshake.write(&[])?).await?;
        } else {
            let frame = read_frame(stream).await?;
            handshake.read(&frame)?;
            check(&handshake)?;
        }
    }
    Ok(handshake.finish()?)
}

/// An authenticated session with a paired peer.
#[derive(Debug)]
pub struct Session<S> {
    pub peer: TrustedPeer,
    /// What the peer announced about itself.
    pub hello: Hello,
    stream: SecureStream<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// # Errors
    /// Fails on I/O or encryption errors.
    pub async fn send(&mut self, body: envelope::Body) -> Result<(), NetError> {
        self.stream.send(&Envelope { body: Some(body) }).await
    }

    /// # Errors
    /// Fails on I/O errors, tampering, or an empty envelope.
    pub async fn recv(&mut self) -> Result<envelope::Body, NetError> {
        let envelope: Envelope = self.stream.recv().await?;
        envelope.body.ok_or(NetError::Protocol("empty envelope"))
    }

    pub fn into_parts(self) -> (TrustedPeer, Hello, SecureStream<S>) {
        (self.peer, self.hello, self.stream)
    }
}

async fn exchange_hello<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut SecureStream<S>,
    ours: Hello,
    initiator: bool,
) -> Result<Hello, NetError> {
    let ours = Envelope {
        body: Some(envelope::Body::Hello(ours)),
    };
    if initiator {
        stream.send(&ours).await?;
    }
    let theirs: Envelope = stream.recv().await?;
    if !initiator {
        stream.send(&ours).await?;
    }
    match theirs.body {
        Some(envelope::Body::Hello(hello)) => Ok(hello),
        _ => Err(NetError::Protocol("expected hello")),
    }
}

/// Opens a session to a paired peer.
///
/// # Errors
/// Fails if the peer cannot prove it holds the trusted key, or on I/O errors.
pub async fn connect_session<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    identity: &DeviceIdentity,
    peer: &TrustedPeer,
    hello: Hello,
) -> Result<Session<S>, NetError> {
    write_preamble(&mut stream, Intent::Session).await?;
    let handshake = Handshake::new(
        identity,
        HandshakeKind::Session(Some(&peer.identity.agreement)),
        Role::Initiator,
    )?;
    let channel = drive(&mut stream, handshake, |_| Ok(())).await?;
    let mut stream = SecureStream::new(stream, channel);
    let hello = exchange_hello(&mut stream, hello, true).await?;
    Ok(Session {
        peer: peer.clone(),
        hello,
        stream,
    })
}

/// Accepts a session after [`crate::read_preamble`] returned
/// [`Intent::Session`]. Unknown initiators are dropped before we send
/// anything, so strangers learn nothing about us.
///
/// # Errors
/// Fails if the initiator is not in `trusted`, or on I/O errors.
pub async fn accept_session<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    identity: &DeviceIdentity,
    trusted: &[TrustedPeer],
    hello: Hello,
) -> Result<Session<S>, NetError> {
    let handshake = Handshake::new(identity, HandshakeKind::Session(None), Role::Responder)?;
    let mut peer = None;
    let channel = drive(&mut stream, handshake, |hs| {
        if peer.is_none()
            && let Some(remote) = hs.remote_static()
        {
            peer = Some(
                trusted
                    .iter()
                    .find(|p| p.identity.agreement == remote)
                    .cloned()
                    .ok_or(NetError::UnknownPeer)?,
            );
        }
        Ok(())
    })
    .await?;
    let peer = peer.ok_or(NetError::UnknownPeer)?;
    let mut stream = SecureStream::new(stream, channel);
    let hello = exchange_hello(&mut stream, hello, false).await?;
    Ok(Session {
        peer,
        hello,
        stream,
    })
}

#[cfg(test)]
mod tests {
    use continuum_core::clock::Timestamp;
    use continuum_core::entropy::DeterministicEntropy;
    use continuum_core::proto::v1::{DeviceInfo, Ping, Pong};

    use super::*;
    use crate::frame::read_preamble;

    fn identity(seed: u64) -> DeviceIdentity {
        DeviceIdentity::generate(&mut DeterministicEntropy::new(seed))
    }

    fn trusted(identity: &DeviceIdentity, name: &str) -> TrustedPeer {
        TrustedPeer {
            identity: identity.public(),
            presence_key: identity.presence_key().clone(),
            name: name.into(),
            paired_at: Timestamp::default(),
        }
    }

    fn hello(name: &str) -> Hello {
        Hello {
            device: Some(DeviceInfo {
                name: name.into(),
                ..Default::default()
            }),
            manifest: None,
            presence_key: Vec::new(),
        }
    }

    #[tokio::test]
    async fn trusted_session_exchanges_hello_and_messages() {
        let (laptop, phone) = (identity(1), identity(2));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let phone_peer = trusted(&phone, "phone");
        let laptop_peer = trusted(&laptop, "laptop");

        let client = async {
            let mut session = connect_session(a, &laptop, &phone_peer, hello("Laptop")).await?;
            session
                .send(envelope::Body::Ping(Ping { nonce: 42 }))
                .await?;
            let reply = session.recv().await?;
            Ok::<_, NetError>((session.hello.clone(), reply))
        };
        let server = async {
            assert_eq!(read_preamble(&mut b).await?, Intent::Session);
            let mut session = accept_session(
                b,
                &phone,
                std::slice::from_ref(&laptop_peer),
                hello("Phone"),
            )
            .await?;
            let envelope::Body::Ping(ping) = session.recv().await? else {
                panic!("expected ping");
            };
            session
                .send(envelope::Body::Pong(Pong { nonce: ping.nonce }))
                .await?;
            Ok::<_, NetError>(session.peer.device_id())
        };
        let (client, server) = tokio::join!(client, server);
        let (their_hello, reply) = client.unwrap();
        assert_eq!(their_hello.device.unwrap().name, "Phone");
        assert_eq!(reply, envelope::Body::Pong(Pong { nonce: 42 }));
        assert_eq!(server.unwrap(), laptop.device_id());
    }

    #[tokio::test]
    async fn unknown_initiator_is_rejected() {
        let (stranger, phone) = (identity(3), identity(2));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let phone_peer = trusted(&phone, "phone");
        let client = connect_session(a, &stranger, &phone_peer, hello("Stranger"));
        let server = async {
            read_preamble(&mut b).await?;
            accept_session(b, &phone, &[], hello("Phone")).await
        };
        let (client, server) = tokio::join!(client, server);
        assert!(matches!(server, Err(NetError::UnknownPeer)));
        assert!(client.is_err());
    }

    #[tokio::test]
    async fn impostor_responder_is_rejected() {
        let (laptop, phone, impostor) = (identity(1), identity(2), identity(4));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let phone_peer = trusted(&phone, "phone");
        let laptop_peer = trusted(&laptop, "laptop");
        let client = connect_session(a, &laptop, &phone_peer, hello("Laptop"));
        let server = async {
            read_preamble(&mut b).await?;
            accept_session(b, &impostor, &[laptop_peer], hello("Impostor")).await
        };
        let (client, server) = tokio::join!(client, server);
        assert!(client.is_err());
        assert!(server.is_err());
    }
}
