//! Trust store: the set of paired peers and the keys needed to recognise
//! and authenticate them.

use std::collections::BTreeMap;
use std::convert::Infallible;

use crate::clock::Timestamp;
use crate::identity::{DeviceId, PublicIdentity};
use crate::rid::PresenceKey;

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
}
