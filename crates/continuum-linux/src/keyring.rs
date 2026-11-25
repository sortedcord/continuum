//! Secret Service (GNOME Keyring, KWallet, ...) storage for the device
//! identity and the trust store.

use continuum_core::entropy::Entropy;
use continuum_core::identity::DeviceIdentity;
use continuum_core::trust::{StorageError, TrustedPeer, decode_peers, encode_peers};

const APPLICATION: &str = "continuum";

#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    #[error("Secret Service: {0}")]
    SecretService(#[from] oo7::Error),
    #[error("stored identity is corrupt")]
    CorruptIdentity,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Keyring items for one Continuum profile. Profiles let several instances
/// run side by side on one machine (useful for testing).
#[derive(Debug)]
pub struct Keyring {
    inner: oo7::Keyring,
    profile: String,
}

impl Keyring {
    /// # Errors
    /// Fails if no Secret Service provider is running.
    pub async fn open(profile: &str) -> Result<Self, KeyringError> {
        let inner = oo7::Keyring::new().await?;
        inner.unlock().await?;
        Ok(Self {
            inner,
            profile: profile.to_owned(),
        })
    }

    fn attributes(&self, kind: &'static str) -> [(&'static str, String); 3] {
        [
            ("application", APPLICATION.to_owned()),
            ("kind", kind.to_owned()),
            ("profile", self.profile.clone()),
        ]
    }

    async fn read(&self, kind: &'static str) -> Result<Option<Vec<u8>>, KeyringError> {
        let items = self.inner.search_items(&self.attributes(kind)).await?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        Ok(Some(item.secret().await?.as_bytes().to_vec()))
    }

    async fn write(
        &self,
        kind: &'static str,
        label: &str,
        secret: &[u8],
    ) -> Result<(), KeyringError> {
        self.inner
            .create_item(label, &self.attributes(kind), secret, true)
            .await?;
        Ok(())
    }

    /// Loads this device's identity, generating and saving one on first use.
    ///
    /// # Errors
    /// Fails if the keyring is unavailable or the stored identity is corrupt.
    pub async fn load_or_create_identity(
        &self,
        entropy: &mut impl Entropy,
    ) -> Result<DeviceIdentity, KeyringError> {
        if let Some(bytes) = self.read("identity").await? {
            return DeviceIdentity::from_secret_bytes(&bytes)
                .map_err(|_| KeyringError::CorruptIdentity);
        }
        let identity = DeviceIdentity::generate(entropy);
        self.save_identity(&identity).await?;
        Ok(identity)
    }

    /// # Errors
    /// Fails if the keyring is unavailable.
    pub async fn save_identity(&self, identity: &DeviceIdentity) -> Result<(), KeyringError> {
        let label = format!("Continuum device identity ({})", self.profile);
        self.write("identity", &label, &*identity.to_secret_bytes())
            .await
    }

    /// # Errors
    /// Fails if the keyring is unavailable or the data is corrupt.
    pub async fn load_peers(&self) -> Result<Vec<TrustedPeer>, KeyringError> {
        match self.read("trust").await? {
            Some(bytes) => Ok(decode_peers(&bytes)?),
            None => Ok(Vec::new()),
        }
    }

    /// # Errors
    /// Fails if the keyring is unavailable.
    pub async fn save_peers(&self, peers: &[TrustedPeer]) -> Result<(), KeyringError> {
        let label = format!("Continuum paired devices ({})", self.profile);
        self.write("trust", &label, &encode_peers(peers)).await
    }

    /// Deletes everything stored for this profile.
    ///
    /// # Errors
    /// Fails if the keyring is unavailable.
    pub async fn reset(&self) -> Result<(), KeyringError> {
        for kind in ["identity", "trust"] {
            self.inner.delete(&self.attributes(kind)).await?;
        }
        Ok(())
    }
}
