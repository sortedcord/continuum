//! Loaded state shared by all commands: identity, trust store, device info.

use std::sync::Arc;

use anyhow::Context as _;
use continuum_core::entropy::OsEntropy;
use continuum_core::identity::{DeviceId, DeviceIdentity};
use continuum_core::proto::PROTOCOL_VERSION;
use continuum_core::proto::v1::{DeviceInfo, DeviceKind};
use continuum_core::trust::TrustedPeer;
use continuum_linux::keyring::Keyring;

pub struct Ctx {
    pub profile: String,
    pub keyring: Keyring,
    pub identity: Arc<DeviceIdentity>,
    pub peers: Vec<TrustedPeer>,
    pub device: DeviceInfo,
}

impl Ctx {
    pub async fn load(profile: &str, name: Option<String>) -> anyhow::Result<Self> {
        let keyring = Keyring::open(profile)
            .await
            .context("opening the Secret Service keyring")?;
        let identity = keyring
            .load_or_create_identity(&mut OsEntropy)
            .await
            .context("loading device identity")?;
        let peers = keyring
            .load_peers()
            .await
            .context("loading paired devices")?;
        let mut device_name = name.unwrap_or_else(host_name);
        if profile != "default" {
            device_name = format!("{device_name} ({profile})");
        }
        Ok(Self {
            profile: profile.to_owned(),
            keyring,
            identity: Arc::new(identity),
            peers,
            device: DeviceInfo {
                name: device_name,
                kind: device_kind().into(),
                protocol_version: PROTOCOL_VERSION,
            },
        })
    }

    /// Adds or replaces a peer and persists the trust store.
    pub async fn save_peer(&mut self, peer: TrustedPeer) -> anyhow::Result<()> {
        let id = peer.device_id();
        self.peers.retain(|existing| existing.device_id() != id);
        self.peers.push(peer);
        self.keyring.save_peers(&self.peers).await?;
        Ok(())
    }

    /// Removes a peer and rotates our presence key so it can no longer
    /// recognise our beacons. Remaining peers learn the new key in their
    /// next session.
    pub async fn remove_peer(&mut self, id: &DeviceId) -> anyhow::Result<()> {
        self.peers.retain(|existing| &existing.device_id() != id);
        self.keyring.save_peers(&self.peers).await?;
        let rotated = self.identity.with_new_presence_key(&mut OsEntropy);
        self.keyring.save_identity(&rotated).await?;
        self.identity = Arc::new(rotated);
        Ok(())
    }

    /// Finds a peer by device id prefix or exact name.
    pub fn find_peer(&self, needle: &str) -> anyhow::Result<&TrustedPeer> {
        match match_peers(&self.peers, needle).as_slice() {
            [peer] => Ok(peer),
            [] => anyhow::bail!("no paired device matches {needle:?}"),
            _ => anyhow::bail!("{needle:?} matches several devices; use a longer id prefix"),
        }
    }
}

/// Finds peers by id prefix, then exact name, then name substring (all
/// case-insensitive), stopping at the first rule that matches anything.
pub fn match_peers<'a>(peers: &'a [TrustedPeer], needle: &str) -> Vec<&'a TrustedPeer> {
    let needle = needle.to_lowercase();
    let rules: [&dyn Fn(&TrustedPeer) -> bool; 3] = [
        &|p| p.device_id().to_string().starts_with(&needle),
        &|p| p.name.to_lowercase() == needle,
        &|p| p.name.to_lowercase().contains(&needle),
    ];
    for rule in rules {
        let found: Vec<_> = peers.iter().filter(|p| rule(p)).collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

pub fn short_id(id: &DeviceId) -> String {
    id.to_string()[..8].to_owned()
}

fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Linux".into())
}

fn device_kind() -> DeviceKind {
    let has_battery = std::fs::read_dir("/sys/class/power_supply").is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("BAT"))
    });
    if has_battery {
        DeviceKind::Laptop
    } else {
        DeviceKind::Desktop
    }
}
