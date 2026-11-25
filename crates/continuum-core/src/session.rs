//! Noise handshakes and encrypted channels (sans-I/O).
//!
//! Three handshake patterns, all over X25519 / ChaCha20-Poly1305 / BLAKE2s,
//! using each device's X25519 agreement key as its Noise static key:
//!
//! | Use | Pattern | Authentication |
//! | --- | --- | --- |
//! | Pairing via QR code | `XXpsk3` | One-time secret from the QR code |
//! | Pairing without a camera | `XX` | Users compare a 6-digit [`Sas`] |
//! | Trusted session | `IK` | Static keys from the trust store |
//!
//! This module only moves bytes in and out. Framing and sockets live in the
//! transport crates.

use std::fmt;

use sha2::{Digest, Sha256};
use x25519_dalek::PublicKey as AgreementPublicKey;

use crate::identity::DeviceIdentity;

const PATTERN_PAIR_PSK: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
const PATTERN_PAIR_SAS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const PATTERN_SESSION: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"continuum/v1";

/// Largest Noise message, including the 16-byte AEAD tag.
pub const MAX_MESSAGE_LEN: usize = 65_535;
/// Largest plaintext that fits in one transport message.
pub const MAX_PLAINTEXT_LEN: usize = MAX_MESSAGE_LEN - 16;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("noise: {0}")]
    Noise(#[from] snow::Error),
    #[error("message too large: {0} bytes")]
    TooLarge(usize),
    #[error("handshake is not finished")]
    Unfinished,
}

/// Which handshake to run.
#[derive(Clone, Copy)]
pub enum HandshakeKind<'a> {
    /// Pairing, authenticated by the QR code's one-time secret.
    PairWithSecret(&'a [u8; 32]),
    /// Pairing, authenticated afterwards by comparing a [`Sas`].
    PairWithSas,
    /// Session between already-paired devices. The initiator must pass the
    /// responder's agreement key; the responder passes `None` and checks the
    /// initiator's key against its trust store after the first message.
    Session(Option<&'a AgreementPublicKey>),
}

impl fmt::Debug for HandshakeKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PairWithSecret(_) => f.write_str("PairWithSecret(<redacted>)"),
            Self::PairWithSas => f.write_str("PairWithSas"),
            Self::Session(remote) => f.debug_tuple("Session").field(&remote.is_some()).finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

pub struct Handshake {
    state: snow::HandshakeState,
}

impl fmt::Debug for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handshake")
            .field("initiator", &self.state.is_initiator())
            .field("finished", &self.state.is_handshake_finished())
            .finish()
    }
}

impl Handshake {
    /// # Errors
    ///
    /// Fails only on an internal Noise configuration error.
    pub fn new(
        identity: &DeviceIdentity,
        kind: HandshakeKind<'_>,
        role: Role,
    ) -> Result<Self, SessionError> {
        let pattern = match kind {
            HandshakeKind::PairWithSecret(_) => PATTERN_PAIR_PSK,
            HandshakeKind::PairWithSas => PATTERN_PAIR_SAS,
            HandshakeKind::Session(_) => PATTERN_SESSION,
        };
        let params = pattern.parse().map_err(SessionError::Noise)?;
        let secret = identity.agreement_secret();
        let mut builder = snow::Builder::new(params)
            .local_private_key(&*secret)?
            .prologue(PROLOGUE)?;
        if let HandshakeKind::PairWithSecret(psk) = kind {
            builder = builder.psk(3, psk)?;
        }
        let remote_bytes;
        if let HandshakeKind::Session(Some(remote)) = kind {
            remote_bytes = *remote.as_bytes();
            builder = builder.remote_public_key(&remote_bytes)?;
        }
        let state = match role {
            Role::Initiator => builder.build_initiator()?,
            Role::Responder => builder.build_responder()?,
        };
        Ok(Self { state })
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    pub fn is_my_turn(&self) -> bool {
        self.state.is_my_turn()
    }

    /// The peer's static key, once the handshake has revealed it.
    pub fn remote_static(&self) -> Option<AgreementPublicKey> {
        remote_key(self.state.get_remote_static())
    }

    /// Produces the next handshake message, carrying an optional payload.
    ///
    /// Payloads in early messages may be unencrypted or not yet
    /// authenticated (see the Noise spec); only send public data in them.
    ///
    /// # Errors
    ///
    /// Fails if it is not our turn or the payload is too large.
    pub fn write(&mut self, payload: &[u8]) -> Result<Vec<u8>, SessionError> {
        let mut out = vec![0; MAX_MESSAGE_LEN];
        let len = self.state.write_message(payload, &mut out)?;
        out.truncate(len);
        Ok(out)
    }

    /// Consumes a handshake message from the peer and returns its payload.
    ///
    /// # Errors
    ///
    /// Fails if the message is malformed or does not authenticate, which
    /// includes a wrong pairing secret or an unexpected static key.
    pub fn read(&mut self, message: &[u8]) -> Result<Vec<u8>, SessionError> {
        let mut out = vec![0; MAX_MESSAGE_LEN];
        let len = self.state.read_message(message, &mut out)?;
        out.truncate(len);
        Ok(out)
    }

    /// # Errors
    ///
    /// Fails if the handshake is not finished.
    pub fn finish(self) -> Result<SecureChannel, SessionError> {
        if !self.state.is_handshake_finished() {
            return Err(SessionError::Unfinished);
        }
        let remote_static =
            remote_key(self.state.get_remote_static()).ok_or(SessionError::Unfinished)?;
        let mut handshake_hash = [0; 32];
        handshake_hash.copy_from_slice(self.state.get_handshake_hash());
        Ok(SecureChannel {
            transport: self.state.into_transport_mode()?,
            remote_static,
            handshake_hash,
        })
    }
}

fn remote_key(bytes: Option<&[u8]>) -> Option<AgreementPublicKey> {
    let bytes: [u8; 32] = bytes?.try_into().ok()?;
    Some(AgreementPublicKey::from(bytes))
}

/// An established, mutually authenticated, forward-secret channel.
pub struct SecureChannel {
    transport: snow::TransportState,
    remote_static: AgreementPublicKey,
    handshake_hash: [u8; 32],
}

impl fmt::Debug for SecureChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureChannel").finish_non_exhaustive()
    }
}

impl SecureChannel {
    pub fn remote_static(&self) -> &AgreementPublicKey {
        &self.remote_static
    }

    /// Unique per handshake; both sides compute the same value.
    pub fn handshake_hash(&self) -> &[u8; 32] {
        &self.handshake_hash
    }

    /// Short authentication string for the pairing-without-camera flow.
    pub fn sas(&self) -> Sas {
        Sas::from_handshake_hash(&self.handshake_hash)
    }

    /// # Errors
    ///
    /// Fails if the plaintext exceeds [`MAX_PLAINTEXT_LEN`].
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > MAX_PLAINTEXT_LEN {
            return Err(SessionError::TooLarge(plaintext.len()));
        }
        let mut out = vec![0; plaintext.len() + 16];
        let len = self.transport.write_message(plaintext, &mut out)?;
        out.truncate(len);
        Ok(out)
    }

    /// # Errors
    ///
    /// Fails if the message was tampered with, replayed or reordered.
    pub fn decrypt(&mut self, message: &[u8]) -> Result<Vec<u8>, SessionError> {
        let mut out = vec![0; message.len()];
        let len = self.transport.read_message(message, &mut out)?;
        out.truncate(len);
        Ok(out)
    }
}

/// Six-digit code both users compare when pairing without a QR code.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sas(u32);

impl Sas {
    fn from_handshake_hash(hash: &[u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"continuum/sas/v1");
        hasher.update(hash);
        let digest = hasher.finalize();
        let mut wide = [0; 8];
        wide.copy_from_slice(&digest[..8]);
        // Modulo bias is below 2^-44.
        let code = u64::from_be_bytes(wide) % 1_000_000;
        Self(u32::try_from(code).unwrap_or_default())
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Formatted as `123 456`.
impl fmt::Display for Sas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:03} {:03}", self.0 / 1000, self.0 % 1000)
    }
}

impl fmt::Debug for Sas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sas({self})")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::entropy::DeterministicEntropy;

    pub(crate) fn identity(seed: u64) -> DeviceIdentity {
        DeviceIdentity::generate(&mut DeterministicEntropy::new(seed))
    }

    /// Runs a handshake to completion in memory.
    pub(crate) fn run(
        initiator: &mut Handshake,
        responder: &mut Handshake,
    ) -> Result<(), SessionError> {
        let (mut sender, mut receiver) = (initiator, responder);
        while !(sender.is_finished() && receiver.is_finished()) {
            let message = sender.write(&[])?;
            receiver.read(&message)?;
            (sender, receiver) = (receiver, sender);
        }
        Ok(())
    }

    fn pair(
        kind_i: HandshakeKind<'_>,
        kind_r: HandshakeKind<'_>,
    ) -> Result<(SecureChannel, SecureChannel), SessionError> {
        let (alice, bob) = (identity(1), identity(2));
        let mut i = Handshake::new(&alice, kind_i, Role::Initiator)?;
        let mut r = Handshake::new(&bob, kind_r, Role::Responder)?;
        run(&mut i, &mut r)?;
        Ok((i.finish()?, r.finish()?))
    }

    #[test]
    fn psk_pairing_succeeds_with_matching_secret() {
        let secret = [7; 32];
        let (mut a, mut b) = pair(
            HandshakeKind::PairWithSecret(&secret),
            HandshakeKind::PairWithSecret(&secret),
        )
        .unwrap();
        assert_eq!(a.remote_static(), &identity(2).public().agreement);
        assert_eq!(b.remote_static(), &identity(1).public().agreement);
        assert_eq!(a.handshake_hash(), b.handshake_hash());

        let ciphertext = a.encrypt(b"hello").unwrap();
        assert_eq!(b.decrypt(&ciphertext).unwrap(), b"hello");
        let reply = b.encrypt(b"hi").unwrap();
        assert_eq!(a.decrypt(&reply).unwrap(), b"hi");
    }

    #[test]
    fn psk_pairing_fails_with_wrong_secret() {
        assert!(
            pair(
                HandshakeKind::PairWithSecret(&[1; 32]),
                HandshakeKind::PairWithSecret(&[2; 32]),
            )
            .is_err()
        );
    }

    #[test]
    fn sas_matches_on_both_sides() {
        let (a, b) = pair(HandshakeKind::PairWithSas, HandshakeKind::PairWithSas).unwrap();
        assert_eq!(a.sas(), b.sas());
        assert!(a.sas().value() < 1_000_000);
        assert_eq!(a.sas().to_string().len(), 7);
    }

    #[test]
    fn session_requires_correct_responder_key() {
        let bob_key = identity(2).public().agreement;
        let (a, b) = pair(
            HandshakeKind::Session(Some(&bob_key)),
            HandshakeKind::Session(None),
        )
        .unwrap();
        assert_eq!(b.remote_static(), &identity(1).public().agreement);
        assert_eq!(a.remote_static(), &bob_key);

        let wrong = identity(3).public().agreement;
        assert!(
            pair(
                HandshakeKind::Session(Some(&wrong)),
                HandshakeKind::Session(None)
            )
            .is_err()
        );
    }

    #[test]
    fn session_responder_learns_initiator_after_first_message() {
        let (alice, bob) = (identity(1), identity(2));
        let bob_key = bob.public().agreement;
        let mut i = Handshake::new(
            &alice,
            HandshakeKind::Session(Some(&bob_key)),
            Role::Initiator,
        )
        .unwrap();
        let mut r = Handshake::new(&bob, HandshakeKind::Session(None), Role::Responder).unwrap();
        r.read(&i.write(&[]).unwrap()).unwrap();
        assert_eq!(r.remote_static(), Some(alice.public().agreement));
    }

    #[test]
    fn tampered_transport_messages_are_rejected() {
        let secret = [9; 32];
        let (mut a, mut b) = pair(
            HandshakeKind::PairWithSecret(&secret),
            HandshakeKind::PairWithSecret(&secret),
        )
        .unwrap();
        let mut ciphertext = a.encrypt(b"payload").unwrap();
        ciphertext[0] ^= 1;
        assert!(b.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn sas_formatting() {
        assert_eq!(Sas(42).to_string(), "000 042");
        assert_eq!(Sas(123_456).to_string(), "123 456");
    }
}
