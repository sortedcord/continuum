//! Pairing: turning a stranger into a [`TrustedPeer`].
//!
//! The device being paired shows an [`Invite`] as a QR code. The other device
//! scans it, connects to one of the listed addresses and runs the
//! `XXpsk3` handshake with the invite's one-time secret (see
//! [`crate::session`]). Without a camera, both sides run plain `XX` and the
//! users compare the [`crate::session::Sas`] instead.
//!
//! Over the resulting channel each side sends a signed [`PairingRecord`].
//! The signature covers the handshake hash, so a record can't be replayed
//! into another handshake, and the record's agreement key must equal the
//! Noise static key, which binds the Ed25519 identity to the channel.

use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use continuum_proto::v1::{DeviceInfo, PairingInvite, PairingRecord};
use ed25519_dalek::{Signature, VerifyingKey};
use prost::Message;
use x25519_dalek::PublicKey as AgreementPublicKey;
use zeroize::Zeroizing;

use crate::clock::Timestamp;
use crate::entropy::Entropy;
use crate::identity::{DeviceIdentity, PublicIdentity};
use crate::rid::PresenceKey;
use crate::session::SecureChannel;
use crate::trust::TrustedPeer;

/// Prefix of the text encoded in a pairing QR code.
pub const INVITE_PREFIX: &str = "continuum:pair:";
const INVITE_VERSION: u32 = 1;
const SIGNATURE_DOMAIN: &[u8] = b"continuum/pair/v1";
/// Longest device name accepted from a peer, in characters.
pub const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("not a Continuum pairing invite")]
    InvalidInvite,
    #[error("unsupported invite version {0}")]
    UnsupportedVersion(u32),
    #[error("pairing invite has expired")]
    Expired,
    #[error("malformed pairing record")]
    MalformedRecord,
    #[error("pairing record key does not match the handshake")]
    KeyMismatch,
    #[error("pairing record signature is invalid")]
    BadSignature,
}

/// What the device being paired shows in its QR code.
#[derive(Clone, PartialEq, Eq)]
pub struct Invite {
    pub addrs: Vec<SocketAddr>,
    pub secret: Zeroizing<[u8; 32]>,
    pub name: String,
    pub expires_at: Timestamp,
}

impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite")
            .field("addrs", &self.addrs)
            .field("name", &self.name)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Invite {
    pub const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);

    pub fn new(
        addrs: Vec<SocketAddr>,
        name: &str,
        now: Timestamp,
        ttl: Duration,
        entropy: &mut impl Entropy,
    ) -> Self {
        Self {
            addrs,
            secret: Zeroizing::new(entropy.array()),
            name: sanitize_name(name),
            expires_at: now.saturating_add(ttl),
        }
    }

    pub fn is_expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }

    /// `continuum:pair:<base64url protobuf>`, compact enough for a QR code.
    pub fn encode(&self) -> String {
        let message = PairingInvite {
            version: INVITE_VERSION,
            addrs: self.addrs.iter().map(ToString::to_string).collect(),
            secret: self.secret.to_vec(),
            name: self.name.clone(),
            expires_at_ms: self.expires_at.as_millis(),
        };
        format!(
            "{INVITE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(message.encode_to_vec())
        )
    }

    /// Parses an invite. Unparseable addresses are skipped.
    ///
    /// # Errors
    ///
    /// Fails if the text is not a valid invite for this protocol version.
    pub fn decode(text: &str) -> Result<Self, PairingError> {
        let body = text
            .trim()
            .strip_prefix(INVITE_PREFIX)
            .ok_or(PairingError::InvalidInvite)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| PairingError::InvalidInvite)?;
        let message =
            PairingInvite::decode(bytes.as_slice()).map_err(|_| PairingError::InvalidInvite)?;
        if message.version != INVITE_VERSION {
            return Err(PairingError::UnsupportedVersion(message.version));
        }
        let secret: [u8; 32] = message
            .secret
            .as_slice()
            .try_into()
            .map_err(|_| PairingError::InvalidInvite)?;
        Ok(Self {
            addrs: message
                .addrs
                .iter()
                .filter_map(|a| a.parse().ok())
                .collect(),
            secret: Zeroizing::new(secret),
            name: sanitize_name(&message.name),
            expires_at: Timestamp::from_millis(message.expires_at_ms),
        })
    }
}

/// Drops control characters and caps the length of a peer-supplied name.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn transcript(handshake_hash: &[u8; 32], agreement: &[u8], presence: &[u8]) -> Vec<u8> {
    [SIGNATURE_DOMAIN, handshake_hash, agreement, presence].concat()
}

/// Builds this device's signed record for the pairing channel.
pub fn make_record(
    identity: &DeviceIdentity,
    device: DeviceInfo,
    channel: &SecureChannel,
) -> PairingRecord {
    let public = identity.public();
    let agreement = public.agreement.as_bytes().to_vec();
    let presence = identity.presence_key().as_bytes().to_vec();
    let signature = identity.sign(&transcript(channel.handshake_hash(), &agreement, &presence));
    PairingRecord {
        signing_key: public.signing.as_bytes().to_vec(),
        agreement_key: agreement,
        presence_key: presence,
        device: Some(device),
        signature: signature.to_bytes().to_vec(),
    }
}

/// Checks the peer's record against the channel it arrived on.
///
/// # Errors
///
/// Fails if the record is malformed, its key doesn't match the handshake,
/// or its signature doesn't verify.
pub fn verify_record(
    record: &PairingRecord,
    channel: &SecureChannel,
    now: Timestamp,
) -> Result<TrustedPeer, PairingError> {
    let bytes32 = |field: &[u8]| -> Result<[u8; 32], PairingError> {
        field.try_into().map_err(|_| PairingError::MalformedRecord)
    };
    let agreement = AgreementPublicKey::from(bytes32(&record.agreement_key)?);
    if &agreement != channel.remote_static() {
        return Err(PairingError::KeyMismatch);
    }
    let signing = VerifyingKey::from_bytes(&bytes32(&record.signing_key)?)
        .map_err(|_| PairingError::MalformedRecord)?;
    let signature =
        Signature::from_slice(&record.signature).map_err(|_| PairingError::MalformedRecord)?;
    let presence_key = PresenceKey::from_bytes(bytes32(&record.presence_key)?);

    let message = transcript(
        channel.handshake_hash(),
        &record.agreement_key,
        &record.presence_key,
    );
    let identity = PublicIdentity { signing, agreement };
    if !identity.verify(&message, &signature) {
        return Err(PairingError::BadSignature);
    }
    let name = record
        .device
        .as_ref()
        .map(|device| sanitize_name(&device.name))
        .unwrap_or_default();
    Ok(TrustedPeer {
        identity,
        presence_key,
        name,
        paired_at: now,
    })
}

#[cfg(test)]
mod tests {
    use continuum_proto::v1::DeviceKind;

    use super::*;
    use crate::entropy::DeterministicEntropy;
    use crate::session::tests::{identity, run};
    use crate::session::{Handshake, HandshakeKind, Role};

    fn info(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            kind: DeviceKind::Laptop.into(),
            protocol_version: 1,
        }
    }

    fn channels(alice: &DeviceIdentity, bob: &DeviceIdentity) -> (SecureChannel, SecureChannel) {
        let secret = [3; 32];
        let kind = HandshakeKind::PairWithSecret(&secret);
        let mut i = Handshake::new(alice, kind, Role::Initiator).unwrap();
        let mut r = Handshake::new(bob, kind, Role::Responder).unwrap();
        run(&mut i, &mut r).unwrap();
        (i.finish().unwrap(), r.finish().unwrap())
    }

    #[test]
    fn invite_round_trip() {
        let mut entropy = DeterministicEntropy::new(1);
        let invite = Invite::new(
            vec![
                "192.168.0.120:47000".parse().unwrap(),
                "[fe80::1]:47000".parse().unwrap(),
            ],
            "motobook",
            Timestamp::from_millis(1_000),
            Invite::DEFAULT_TTL,
            &mut entropy,
        );
        let text = invite.encode();
        assert!(text.starts_with(INVITE_PREFIX));
        assert!(
            text.len() < 200,
            "should fit a small QR code: {}",
            text.len()
        );
        assert_eq!(Invite::decode(&text).unwrap(), invite);
        assert!(!invite.is_expired(Timestamp::from_millis(1_000)));
        assert!(invite.is_expired(Timestamp::from_millis(301_000)));
    }

    #[test]
    fn rejects_bad_invites() {
        assert_eq!(Invite::decode("hello"), Err(PairingError::InvalidInvite));
        assert_eq!(
            Invite::decode("continuum:pair:!!!"),
            Err(PairingError::InvalidInvite)
        );
        let future = PairingInvite {
            version: 9,
            ..Default::default()
        };
        let text = format!(
            "{INVITE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(future.encode_to_vec())
        );
        assert_eq!(
            Invite::decode(&text),
            Err(PairingError::UnsupportedVersion(9))
        );
    }

    #[test]
    fn records_verify_both_ways() {
        let (alice, bob) = (identity(1), identity(2));
        let (a, b) = channels(&alice, &bob);
        let now = Timestamp::from_millis(5);

        let from_alice = verify_record(&make_record(&alice, info("Alice"), &a), &b, now).unwrap();
        assert_eq!(from_alice.device_id(), alice.device_id());
        assert_eq!(&from_alice.presence_key, alice.presence_key());
        assert_eq!(from_alice.name, "Alice");

        let from_bob = verify_record(&make_record(&bob, info("Bob"), &b), &a, now).unwrap();
        assert_eq!(from_bob.device_id(), bob.device_id());
    }

    #[test]
    fn record_from_another_handshake_is_rejected() {
        let (alice, bob) = (identity(1), identity(2));
        let (a1, _) = channels(&alice, &bob);
        let (_, b2) = channels(&alice, &bob);
        let record = make_record(&alice, info("Alice"), &a1);
        assert_eq!(
            verify_record(&record, &b2, Timestamp::default()),
            Err(PairingError::BadSignature)
        );
    }

    #[test]
    fn record_for_a_different_key_is_rejected() {
        let (alice, bob, mallory) = (identity(1), identity(2), identity(3));
        let (a, b) = channels(&alice, &bob);
        let record = make_record(&mallory, info("Mallory"), &a);
        assert_eq!(
            verify_record(&record, &b, Timestamp::default()),
            Err(PairingError::KeyMismatch)
        );
    }

    #[test]
    fn tampered_presence_key_is_rejected() {
        let (alice, bob) = (identity(1), identity(2));
        let (a, b) = channels(&alice, &bob);
        let mut record = make_record(&alice, info("Alice"), &a);
        record.presence_key[0] ^= 1;
        assert_eq!(
            verify_record(&record, &b, Timestamp::default()),
            Err(PairingError::BadSignature)
        );
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("  Pixel\u{7}\n 9 "), "Pixel 9");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), MAX_NAME_CHARS);
    }
}
