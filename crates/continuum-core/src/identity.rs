//! Long-term device identity.
//!
//! Every device has three secrets, generated once on first run:
//!
//! * an Ed25519 signing key, which *is* the device's identity,
//! * an X25519 static key for session key agreement,
//! * a [`PresenceKey`] for rotating discovery identifiers.
//!
//! The [`DeviceId`] is a hash of the signing key, so it cannot be forged
//! without the private key. It is never broadcast; it only appears inside
//! authenticated sessions and the local trust store.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as AgreementPublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::entropy::Entropy;
use crate::rid::PresenceKey;

const DEVICE_ID_DOMAIN: &[u8] = b"continuum/device-id/v1";

/// Stable, unforgeable name for a device: a truncated hash of its signing key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId([u8; 16]);

impl DeviceId {
    pub fn for_signing_key(key: &VerifyingKey) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DEVICE_ID_DOMAIN);
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let mut id = [0; 16];
        id.copy_from_slice(&digest[..16]);
        Self(id)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({self})")
    }
}

/// The public half of a device identity, as stored for trusted peers.
#[derive(Clone, PartialEq, Eq)]
pub struct PublicIdentity {
    pub signing: VerifyingKey,
    pub agreement: AgreementPublicKey,
}

impl PublicIdentity {
    pub fn device_id(&self) -> DeviceId {
        DeviceId::for_signing_key(&self.signing)
    }

    /// Verifies a signature made with [`DeviceIdentity::sign`].
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.signing.verify(message, signature).is_ok()
    }
}

impl fmt::Debug for PublicIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicIdentity")
            .field("device_id", &self.device_id())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("identity secrets must be {} bytes", DeviceIdentity::SECRET_LEN)]
    WrongSecretLength,
}

/// This device's own identity, including private keys.
pub struct DeviceIdentity {
    signing: SigningKey,
    agreement: StaticSecret,
    presence: PresenceKey,
}

impl DeviceIdentity {
    /// Length of [`Self::to_secret_bytes`]: three 32-byte secrets.
    pub const SECRET_LEN: usize = 96;

    pub fn generate(entropy: &mut impl Entropy) -> Self {
        let signing = SigningKey::from_bytes(&Zeroizing::new(entropy.array()));
        let agreement = StaticSecret::from(*Zeroizing::new(entropy.array::<32>()));
        let presence = PresenceKey::generate(entropy);
        Self {
            signing,
            agreement,
            presence,
        }
    }

    /// Serialises all secrets for the platform keystore (Secret Service on
    /// Linux, Android Keystore-wrapped storage on Android).
    pub fn to_secret_bytes(&self) -> Zeroizing<[u8; Self::SECRET_LEN]> {
        let mut out = Zeroizing::new([0; Self::SECRET_LEN]);
        out[..32].copy_from_slice(self.signing.as_bytes());
        out[32..64].copy_from_slice(self.agreement.as_bytes());
        out[64..].copy_from_slice(self.presence.as_bytes());
        out
    }

    /// Restores an identity saved with [`Self::to_secret_bytes`].
    ///
    /// # Errors
    ///
    /// Fails if `bytes` is not exactly [`Self::SECRET_LEN`] long.
    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() != Self::SECRET_LEN {
            return Err(IdentityError::WrongSecretLength);
        }
        let part = |range: std::ops::Range<usize>| {
            let mut out = Zeroizing::new([0u8; 32]);
            out.copy_from_slice(&bytes[range]);
            out
        };
        Ok(Self {
            signing: SigningKey::from_bytes(&part(0..32)),
            agreement: StaticSecret::from(*part(32..64)),
            presence: PresenceKey::from_bytes(*part(64..96)),
        })
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::for_signing_key(&self.signing.verifying_key())
    }

    pub fn public(&self) -> PublicIdentity {
        PublicIdentity {
            signing: self.signing.verifying_key(),
            agreement: AgreementPublicKey::from(&self.agreement),
        }
    }

    pub fn presence_key(&self) -> &PresenceKey {
        &self.presence
    }

    /// Replaces the presence key, e.g. after revoking a peer. The new key
    /// must be redistributed to every remaining trusted peer.
    pub fn rotate_presence_key(&mut self, entropy: &mut impl Entropy) {
        self.presence = PresenceKey::generate(entropy);
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing.sign(message)
    }

    /// Raw X25519 private key, for the Noise handshake.
    pub(crate) fn agreement_secret(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.agreement.to_bytes())
    }

    /// Static-static Diffie-Hellman with a peer. Session handshakes build on
    /// this; it is not meant to be used as a key directly.
    pub fn agree(&self, peer: &AgreementPublicKey) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(*self.agreement.diffie_hellman(peer).as_bytes())
    }
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("device_id", &self.device_id())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::DeterministicEntropy;

    fn identity(seed: u64) -> DeviceIdentity {
        DeviceIdentity::generate(&mut DeterministicEntropy::new(seed))
    }

    #[test]
    fn secrets_round_trip() {
        let original = identity(1);
        let restored = DeviceIdentity::from_secret_bytes(&*original.to_secret_bytes()).unwrap();
        assert_eq!(restored.device_id(), original.device_id());
        assert_eq!(restored.public(), original.public());
        assert_eq!(restored.presence_key(), original.presence_key());
    }

    #[test]
    fn rejects_truncated_secrets() {
        assert_eq!(
            DeviceIdentity::from_secret_bytes(&[0; 95]).unwrap_err(),
            IdentityError::WrongSecretLength
        );
    }

    #[test]
    fn device_id_matches_public_identity() {
        let id = identity(2);
        assert_eq!(id.public().device_id(), id.device_id());
        assert_ne!(identity(3).device_id(), id.device_id());
    }

    #[test]
    fn signatures_verify_only_for_signer() {
        let (alice, bob) = (identity(4), identity(5));
        let signature = alice.sign(b"hello");
        assert!(alice.public().verify(b"hello", &signature));
        assert!(!alice.public().verify(b"hullo", &signature));
        assert!(!bob.public().verify(b"hello", &signature));
    }

    #[test]
    fn key_agreement_is_symmetric() {
        let (alice, bob) = (identity(6), identity(7));
        let ab = alice.agree(&bob.public().agreement);
        let ba = bob.agree(&alice.public().agreement);
        assert_eq!(*ab, *ba);
    }

    #[test]
    fn debug_output_hides_secrets() {
        let text = format!("{:?}", identity(8));
        assert!(text.contains("device_id"));
        assert!(!text.contains("signing"));
    }
}
