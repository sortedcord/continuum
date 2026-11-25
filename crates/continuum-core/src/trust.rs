//! Trust store: the set of paired peers and the keys needed to recognise
//! and authenticate them.

use std::collections::BTreeMap;
use std::convert::Infallible;

use continuum_proto::v1::{StoredPeer, StoredTrust};
use ed25519_dalek::VerifyingKey;
use prost::Message;
use x25519_dalek::PublicKey as AgreementPublicKey;
use zeroize::Zeroizing;

use crate::clock::Timestamp;
use crate::identity::{DeviceId, PublicIdentity};
use crate::rid::PresenceKey;

const STORAGE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    #[error("stored trust data is corrupt")]
    Corrupt,
    #[error("unsupported trust storage version {0}")]
    UnsupportedVersion(u32),
}

/// Serialises peers for the platform keystore. The output contains presence
/// keys, so treat it as a secret.
pub fn encode_peers<'a>(peers: impl IntoIterator<Item = &'a TrustedPeer>) -> Zeroizing<Vec<u8>> {
    let stored = StoredTrust {
        version: STORAGE_VERSION,
        peers: peers
            .into_iter()
            .map(|peer| StoredPeer {
                signing_key: peer.identity.signing.as_bytes().to_vec(),
                agreement_key: peer.identity.agreement.as_bytes().to_vec(),
                presence_key: peer.presence_key.as_bytes().to_vec(),
                name: peer.name.clone(),
                paired_at_ms: peer.paired_at.as_millis(),
            })
            .collect(),
    };
    Zeroizing::new(stored.encode_to_vec())
}

/// # Errors
/// Fails if the data is corrupt or from a newer version.
pub fn decode_peers(bytes: &[u8]) -> Result<Vec<TrustedPeer>, StorageError> {
    let stored = StoredTrust::decode(bytes).map_err(|_| StorageError::Corrupt)?;
    if stored.version != STORAGE_VERSION {
        return Err(StorageError::UnsupportedVersion(stored.version));
    }
    let key = |bytes: &[u8]| -> Result<[u8; 32], StorageError> {
        bytes.try_into().map_err(|_| StorageError::Corrupt)
    };
    stored
        .peers
        .iter()
        .map(|peer| {
            Ok(TrustedPeer {
                identity: PublicIdentity {
                    signing: VerifyingKey::from_bytes(&key(&peer.signing_key)?)
                        .map_err(|_| StorageError::Corrupt)?,
                    agreement: AgreementPublicKey::from(key(&peer.agreement_key)?),
                },
                presence_key: PresenceKey::from_bytes(key(&peer.presence_key)?),
                name: peer.name.clone(),
                paired_at: Timestamp::from_millis(peer.paired_at_ms),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPeer {
    pub identity: PublicIdentity,
    /// The peer's presence key, received during pairing.
    pub presence_key: PresenceKey,
    /// User-facing name, editable locally. Never broadcast.
    pub name: String,
    pub paired_at: Timestamp,
}

impl TrustedPeer {
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }
}

/// Persistent storage for trusted peers. Platform adapters back this with
/// the system keyring; [`MemoryTrustStore`] is for tests.
pub trait TrustStore {
    type Error: std::error::Error + Send + Sync + 'static;

    /// # Errors
    /// Fails if the backing store cannot be read.
    fn list(&self) -> Result<Vec<TrustedPeer>, Self::Error>;

    /// # Errors
    /// Fails if the backing store cannot be read.
    fn get(&self, id: &DeviceId) -> Result<Option<TrustedPeer>, Self::Error>;

    /// Adds or replaces a peer.
    ///
    /// # Errors
    /// Fails if the backing store cannot be written.
    fn upsert(&mut self, peer: TrustedPeer) -> Result<(), Self::Error>;

    /// # Errors
    /// Fails if the backing store cannot be written.
    fn remove(&mut self, id: &DeviceId) -> Result<Option<TrustedPeer>, Self::Error>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryTrustStore {
    peers: BTreeMap<DeviceId, TrustedPeer>,
}

impl TrustStore for MemoryTrustStore {
    type Error = Infallible;

    fn list(&self) -> Result<Vec<TrustedPeer>, Self::Error> {
        Ok(self.peers.values().cloned().collect())
    }

    fn get(&self, id: &DeviceId) -> Result<Option<TrustedPeer>, Self::Error> {
        Ok(self.peers.get(id).cloned())
    }

    fn upsert(&mut self, peer: TrustedPeer) -> Result<(), Self::Error> {
        self.peers.insert(peer.device_id(), peer);
        Ok(())
    }

    fn remove(&mut self, id: &DeviceId) -> Result<Option<TrustedPeer>, Self::Error> {
        Ok(self.peers.remove(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::DeterministicEntropy;
    use crate::identity::DeviceIdentity;

    fn peer(seed: u64) -> TrustedPeer {
        let identity = DeviceIdentity::generate(&mut DeterministicEntropy::new(seed));
        TrustedPeer {
            identity: identity.public(),
            presence_key: identity.presence_key().clone(),
            name: format!("peer-{seed}"),
            paired_at: Timestamp::from_millis(0),
        }
    }

    #[test]
    fn upsert_get_remove() {
        let mut store = MemoryTrustStore::default();
        let alice = peer(1);
        let id = alice.device_id();

        store.upsert(alice.clone()).unwrap();
        assert_eq!(store.get(&id).unwrap(), Some(alice.clone()));
        assert_eq!(store.list().unwrap().len(), 1);

        let renamed = TrustedPeer {
            name: "Phone".into(),
            ..alice
        };
        store.upsert(renamed.clone()).unwrap();
        assert_eq!(store.list().unwrap(), vec![renamed.clone()]);

        assert_eq!(store.remove(&id).unwrap(), Some(renamed));
        assert_eq!(store.get(&id).unwrap(), None);
    }

    #[test]
    fn storage_round_trip() {
        let peers = vec![peer(1), peer(2)];
        let bytes = encode_peers(&peers);
        assert_eq!(decode_peers(&bytes).unwrap(), peers);
        assert_eq!(decode_peers(&encode_peers(&[])).unwrap(), vec![]);
    }

    #[test]
    fn storage_rejects_garbage() {
        assert_eq!(decode_peers(&[0xff, 0xff]), Err(StorageError::Corrupt));
        let future = StoredTrust {
            version: 2,
            peers: vec![],
        };
        assert_eq!(
            decode_peers(&future.encode_to_vec()),
            Err(StorageError::UnsupportedVersion(2))
        );
    }
}
