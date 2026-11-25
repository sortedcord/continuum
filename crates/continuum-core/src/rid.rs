//! Rotating resolvable identifiers.
//!
//! Each device owns one [`PresenceKey`] and shares it with every device it
//! pairs with. While advertising (over BLE or mDNS) it publishes
//!
//! ```text
//! id = nonce || HMAC-SHA256(presence_key, DOMAIN || nonce)[..6]
//! ```
//!
//! with a fresh random `nonce` on every rotation. A trusted peer resolves an
//! identifier by checking the tag against each presence key it holds. To
//! anyone else, successive identifiers are unlinkable random bytes.
//!
//! Resolution needs no clock synchronisation: the nonce travels with the tag.
//!
//! Known limits:
//! * Identifiers can be replayed, so a sighting proves nothing on its own.
//!   Trust always comes from the session handshake.
//! * Revoking one peer means rotating the presence key and giving the new
//!   key to the remaining peers.

use std::fmt;
use std::time::Duration;

use continuum_proto::ResolvableId;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::clock::Timestamp;
use crate::entropy::Entropy;

const DOMAIN: &[u8] = b"continuum/rid/v1";

/// Secret that lets trusted peers recognise a device's rotating identifiers.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct PresenceKey([u8; 32]);

impl PresenceKey {
    pub const LEN: usize = 32;

    pub fn generate(entropy: &mut impl Entropy) -> Self {
        Self(entropy.array())
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    fn mac(&self, nonce: [u8; ResolvableId::NONCE_LEN]) -> Hmac<Sha256> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts keys of any length");
        mac.update(DOMAIN);
        mac.update(&nonce);
        mac
    }

    /// Builds the identifier for a given nonce.
    pub fn derive(&self, nonce: [u8; ResolvableId::NONCE_LEN]) -> ResolvableId {
        let digest = self.mac(nonce).finalize().into_bytes();
        let mut tag = [0; ResolvableId::TAG_LEN];
        tag.copy_from_slice(&digest[..ResolvableId::TAG_LEN]);
        ResolvableId { nonce, tag }
    }

    /// Builds an identifier with a fresh random nonce.
    pub fn generate_id(&self, entropy: &mut impl Entropy) -> ResolvableId {
        self.derive(entropy.array())
    }

    /// Returns whether `id` was produced with this key (constant-time).
    pub fn resolves(&self, id: &ResolvableId) -> bool {
        self.mac(id.nonce).verify_truncated_left(&id.tag).is_ok()
    }
}

impl fmt::Debug for PresenceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PresenceKey(<redacted>)")
    }
}

/// An identifier that cannot be resolved by anyone, for pairing mode.
pub fn random_id(entropy: &mut impl Entropy) -> ResolvableId {
    ResolvableId::from_bytes(entropy.array())
}

/// Hands out the current identifier and rotates it on a fixed interval.
#[derive(Debug)]
pub struct RotatingId {
    key: PresenceKey,
    interval: Duration,
    current: Option<(ResolvableId, Timestamp)>,
}

impl RotatingId {
    /// Same order as BLE private address rotation on Linux and Android.
    pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(15 * 60);

    pub fn new(key: PresenceKey, interval: Duration) -> Self {
        Self {
            key,
            interval,
            current: None,
        }
    }

    /// Returns the identifier to advertise at `now`, rotating it if the
    /// interval has elapsed. The flag is `true` when the identifier changed,
    /// so adapters know to restart their advertisement.
    pub fn current(&mut self, now: Timestamp, entropy: &mut impl Entropy) -> (ResolvableId, bool) {
        if let Some((id, issued)) = self.current
            && now.saturating_since(issued) < self.interval
        {
            return (id, false);
        }
        let id = self.key.generate_id(entropy);
        self.current = Some((id, now));
        (id, true)
    }

    /// Forces a new identifier on the next call to [`Self::current`], e.g.
    /// after the BLE address changed or the network changed.
    pub fn invalidate(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::DeterministicEntropy;

    fn key(seed: u64) -> PresenceKey {
        PresenceKey::generate(&mut DeterministicEntropy::new(seed))
    }

    #[test]
    fn owner_key_resolves_its_ids() {
        let mut entropy = DeterministicEntropy::new(1);
        let key = key(42);
        for _ in 0..100 {
            assert!(key.resolves(&key.generate_id(&mut entropy)));
        }
    }

    #[test]
    fn other_keys_do_not_resolve() {
        let mut entropy = DeterministicEntropy::new(1);
        let (mine, theirs) = (key(1), key(2));
        for _ in 0..100 {
            assert!(!theirs.resolves(&mine.generate_id(&mut entropy)));
        }
    }

    #[test]
    fn tampered_ids_do_not_resolve() {
        let key = key(3);
        let mut id = key.derive([9; 6]);
        id.tag[0] ^= 1;
        assert!(!key.resolves(&id));
    }

    #[test]
    fn derivation_is_stable() {
        // Known answer, cross-checked with Python's hmac module. Changing it
        // is a protocol break.
        let id = PresenceKey::from_bytes([0x11; 32]).derive([0x22; 6]);
        assert_eq!(id.to_string(), "222222222222b7b2f35dcbc8");
    }

    #[test]
    fn successive_ids_differ() {
        let mut entropy = DeterministicEntropy::new(5);
        let key = key(5);
        assert_ne!(key.generate_id(&mut entropy), key.generate_id(&mut entropy));
    }

    #[test]
    fn rotation_follows_interval() {
        let mut entropy = DeterministicEntropy::new(6);
        let mut rotating = RotatingId::new(key(6), Duration::from_secs(60));
        let t0 = Timestamp::from_millis(0);

        let (first, changed) = rotating.current(t0, &mut entropy);
        assert!(changed);
        let (same, changed) =
            rotating.current(t0.saturating_add(Duration::from_secs(59)), &mut entropy);
        assert!(!changed);
        assert_eq!(same, first);
        let (next, changed) =
            rotating.current(t0.saturating_add(Duration::from_secs(60)), &mut entropy);
        assert!(changed);
        assert_ne!(next, first);
    }

    #[test]
    fn invalidate_forces_rotation() {
        let mut entropy = DeterministicEntropy::new(7);
        let mut rotating = RotatingId::new(key(7), RotatingId::DEFAULT_INTERVAL);
        let now = Timestamp::from_millis(0);
        let (first, _) = rotating.current(now, &mut entropy);
        rotating.invalidate();
        let (second, changed) = rotating.current(now, &mut entropy);
        assert!(changed);
        assert_ne!(first, second);
    }
}
