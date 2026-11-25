//! D-Bus API of the running daemon: `dev.continuum.Daemon1` on the session
//! bus. The CLI uses it, and so can desktop shells (tray icons, bars).

use std::sync::{Arc, Mutex, PoisonError};

use continuum_core::entropy::OsEntropy;
use continuum_core::identity::DeviceIdentity;
use continuum_core::presence::ProximityBand;
use continuum_core::trust::TrustedPeer;
use continuum_linux::keyring::Keyring;
use continuum_node::features::page::{self, PageAction};
use continuum_node::{NodeHandle, PeerStatus};
use serde::{Deserialize, Serialize};
use zbus::object_server::SignalEmitter;

pub const OBJECT_PATH: &str = "/dev/continuum/Daemon";

/// Well-known bus name for a profile.
pub fn bus_name(profile: &str) -> String {
    if profile == "default" {
        return "dev.continuum.Daemon".to_owned();
    }
    let element: String = profile
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("dev.continuum.Daemon.p_{element}")
}

/// One paired device, as reported over D-Bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, zbus::Type)]
#[allow(clippy::struct_excessive_bools)] // Flat for D-Bus consumers.
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub reachable: bool,
    pub nearby: bool,
    /// `immediate`, `near`, `far`, or empty when not nearby.
    pub proximity: String,
    /// Smoothed RSSI in dBm, or 0 when not nearby.
    pub rssi: f64,
    pub can_page: bool,
}

impl From<&PeerStatus> for PeerInfo {
    fn from(status: &PeerStatus) -> Self {
        Self {
            id: status.id.to_string(),
            name: status.name.clone(),
            connected: status.connected,
            reachable: status.reachable,
            nearby: status.nearby,
            proximity: match status.band {
                Some(ProximityBand::Immediate) => "immediate",
                Some(ProximityBand::Near) => "near",
                Some(ProximityBand::Far) => "far",
                None => "",
            }
            .to_owned(),
            rssi: status.rssi.map_or(0.0, f64::from),
            can_page: status.capabilities.contains_key(page::CAPABILITY),
        }
    }
}

/// State shared between the daemon loop and the D-Bus interface.
pub struct Shared {
    pub node: NodeHandle,
    pub keyring: Keyring,
    pub identity: Mutex<Arc<DeviceIdentity>>,
    pub peers: Mutex<Vec<TrustedPeer>>,
    pub name: String,
}

impl Shared {
    pub fn peers(&self) -> Vec<TrustedPeer> {
        self.peers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_peers(&self, peers: Vec<TrustedPeer>) {
        *self.peers.lock().unwrap_or_else(PoisonError::into_inner) = peers;
    }

    pub fn peer_name(&self, id: &continuum_core::identity::DeviceId) -> String {
        self.peers()
            .iter()
            .find(|p| &p.device_id() == id)
            .map_or_else(|| crate::ctx::short_id(id), |p| p.name.clone())
    }

    fn find(&self, needle: &str) -> zbus::fdo::Result<TrustedPeer> {
        let peers = self.peers();
        match crate::ctx::match_peers(&peers, needle).as_slice() {
            [peer] => Ok((*peer).clone()),
            [] => Err(zbus::fdo::Error::InvalidArgs(format!(
                "no paired device matches {needle:?}"
            ))),
            _ => Err(zbus::fdo::Error::InvalidArgs(format!(
                "{needle:?} matches several devices; use a longer id prefix"
            ))),
        }
    }

    /// Unpairs a device on both sides, rotates our presence key and saves.
    pub async fn unpair(&self, peer: &TrustedPeer) -> anyhow::Result<()> {
        let id = peer.device_id();
        self.node.unpair(id).await.ok();
        let mut peers = self.peers();
        peers.retain(|p| p.device_id() != id);
        self.keyring.save_peers(&peers).await?;
        self.set_peers(peers);
        let rotated = Arc::new(self.identity().with_new_presence_key(&mut OsEntropy));
        self.keyring.save_identity(&rotated).await?;
        *self.identity.lock().unwrap_or_else(PoisonError::into_inner) = rotated.clone();
        self.node.set_identity(rotated).await?;
        Ok(())
    }

    fn identity(&self) -> Arc<DeviceIdentity> {
        self.identity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn failed(err: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(err.to_string())
}

pub struct DaemonService {
    pub shared: Arc<Shared>,
}

#[zbus::interface(name = "dev.continuum.Daemon1")]
impl DaemonService {
    #[zbus(property)]
    fn device_id(&self) -> String {
        self.shared.identity().device_id().to_string()
    }

    #[zbus(property)]
    fn name(&self) -> String {
        self.shared.name.clone()
    }

    /// Every paired device with its current presence.
    async fn peers(&self) -> zbus::fdo::Result<Vec<PeerInfo>> {
        let statuses = self.shared.node.snapshot().await.map_err(failed)?;
        Ok(statuses.iter().map(PeerInfo::from).collect())
    }

    /// Makes a device ring for `seconds` (0 = default). Returns its id.
    async fn page(&self, device: String, seconds: u32) -> zbus::fdo::Result<String> {
        let peer = self.shared.find(&device)?;
        self.shared
            .node
            .send_feature(
                peer.device_id(),
                page::CAPABILITY,
                page::encode(PageAction::Start {
                    duration_s: seconds,
                }),
            )
            .await
            .map_err(failed)?;
        Ok(peer.device_id().to_string())
    }

    async fn stop_page(&self, device: String) -> zbus::fdo::Result<()> {
        let peer = self.shared.find(&device)?;
        self.shared
            .node
            .send_feature(
                peer.device_id(),
                page::CAPABILITY,
                page::encode(PageAction::Stop),
            )
            .await
            .map_err(failed)
    }

    /// Unpairs a device on both sides. Returns its name.
    async fn unpair(&self, device: String) -> zbus::fdo::Result<String> {
        let peer = self.shared.find(&device)?;
        self.shared.unpair(&peer).await.map_err(failed)?;
        Ok(peer.name)
    }

    #[zbus(signal)]
    pub async fn peer_changed(emitter: &SignalEmitter<'_>, peer: PeerInfo) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn page_found(emitter: &SignalEmitter<'_>, id: &str, name: &str) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "dev.continuum.Daemon1",
    default_path = "/dev/continuum/Daemon"
)]
pub trait Daemon {
    fn peers(&self) -> zbus::Result<Vec<PeerInfo>>;
    fn page(&self, device: &str, seconds: u32) -> zbus::Result<String>;
    fn stop_page(&self, device: &str) -> zbus::Result<()>;
    fn unpair(&self, device: &str) -> zbus::Result<String>;

    #[zbus(signal)]
    fn peer_changed(&self, peer: PeerInfo) -> zbus::Result<()>;

    #[zbus(signal)]
    fn page_found(&self, id: String, name: String) -> zbus::Result<()>;
}

/// Connects to the running daemon for `profile`, if any.
pub async fn connect(profile: &str) -> anyhow::Result<DaemonProxy<'static>> {
    let conn = zbus::Connection::session().await?;
    let proxy = DaemonProxy::builder(&conn)
        .destination(bus_name(profile))
        .build()
        .await?;
    // Fail fast with a clear message if nobody owns the name.
    proxy.peers().await.map_err(|_| {
        anyhow::anyhow!("the Continuum daemon is not running (start it with `continuum run`)")
    })?;
    Ok(proxy)
}
