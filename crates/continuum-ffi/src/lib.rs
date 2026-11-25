//! UniFFI bindings for the Continuum node.
//!
//! The Kotlin side owns persistence and the platform radios; Rust owns the
//! protocol. All methods are synchronous: quick ones return immediately,
//! pairing blocks (call it off the main thread). Events arrive on a
//! [`NodeListener`] from a background thread.

// UniFFI passes arguments by value across the FFI boundary.
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use continuum_core::clock::SystemClock;
use continuum_core::entropy::OsEntropy;
use continuum_core::identity::{DeviceId, DeviceIdentity};
use continuum_core::presence::ProximityBand;
use continuum_core::proto::v1::{DeviceInfo, DeviceKind};
use continuum_core::proto::{BLE_SERVICE_UUID, PROTOCOL_VERSION};
use continuum_core::trust::{TrustedPeer, decode_peers, encode_peers};
use continuum_node::features::page::{self, PageAction};
use continuum_node::pairing::{join_invite, join_with_code};
use continuum_node::{NodeConfig, NodeEvent, NodeHandle, PeerStatus};
use tokio::runtime::Runtime;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ContinuumError {
    #[error("invalid input: {detail}")]
    Invalid { detail: String },
    #[error("pairing failed: {detail}")]
    Pairing { detail: String },
    #[error("network error: {detail}")]
    Network { detail: String },
    #[error("the node is not running")]
    NotRunning,
}

fn invalid(err: impl std::fmt::Display) -> ContinuumError {
    ContinuumError::Invalid {
        detail: err.to_string(),
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum DeviceKindFfi {
    Unknown,
    Phone,
    Tablet,
    Laptop,
    Desktop,
}

impl From<DeviceKindFfi> for DeviceKind {
    fn from(kind: DeviceKindFfi) -> Self {
        match kind {
            DeviceKindFfi::Unknown => Self::Unspecified,
            DeviceKindFfi::Phone => Self::Phone,
            DeviceKindFfi::Tablet => Self::Tablet,
            DeviceKindFfi::Laptop => Self::Laptop,
            DeviceKindFfi::Desktop => Self::Desktop,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum ProximityFfi {
    Immediate,
    Near,
    Far,
}

impl From<ProximityBand> for ProximityFfi {
    fn from(band: ProximityBand) -> Self {
        match band {
            ProximityBand::Immediate => Self::Immediate,
            ProximityBand::Near => Self::Near,
            ProximityBand::Far => Self::Far,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PairedDevice {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub paired_at_ms: u64,
}

impl From<&TrustedPeer> for PairedDevice {
    fn from(peer: &TrustedPeer) -> Self {
        let id = peer.device_id().to_string();
        Self {
            short_id: id[..8].to_owned(),
            id,
            name: peer.name.clone(),
            paired_at_ms: peer.paired_at.as_millis(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
#[allow(clippy::struct_excessive_bools)] // A flat record is simplest for Kotlin.
pub struct PeerStatusFfi {
    pub id: String,
    pub name: String,
    pub nearby: bool,
    pub reachable: bool,
    pub connected: bool,
    pub proximity: Option<ProximityFfi>,
    pub rssi: Option<f32>,
    pub addrs: Vec<String>,
    /// Whether the peer can be paged right now.
    pub can_page: bool,
}

impl From<PeerStatus> for PeerStatusFfi {
    fn from(status: PeerStatus) -> Self {
        Self {
            id: status.id.to_string(),
            name: status.name,
            nearby: status.nearby,
            reachable: status.reachable,
            connected: status.connected,
            proximity: status.band.map(Into::into),
            rssi: status.rssi,
            addrs: status.addrs.iter().map(ToString::to_string).collect(),
            can_page: status.capabilities.contains_key(page::CAPABILITY),
        }
    }
}

/// What the app should publish right now. Replaces any previous one.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AdvertisementFfi {
    /// 128-bit service UUID for the BLE service data.
    pub ble_service_uuid: String,
    /// BLE service data.
    pub beacon: Vec<u8>,
    /// DNS-SD instance name.
    pub service_name: String,
    /// DNS-SD type for NsdManager, e.g. `_continuum._tcp`.
    pub service_type: String,
    pub port: u16,
    pub txt: HashMap<String, String>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum NodeEventFfi {
    Advertise {
        advertisement: AdvertisementFfi,
    },
    Peer {
        status: PeerStatusFfi,
    },
    SessionUp {
        peer_id: String,
        name: String,
    },
    SessionDown {
        peer_id: String,
        reason: String,
    },
    PairingCandidate {
        addr: Option<String>,
        present: bool,
    },
    /// The trust store changed (a peer rotated its key, or unpaired us).
    /// Persist [`Continuum::trust_store`].
    TrustChanged,
    /// A peer unpaired us.
    Unpaired {
        peer_id: String,
        name: String,
    },
    /// A peer wants this device to ring, even on silent.
    PageStart {
        peer_id: String,
        name: String,
        duration_s: u32,
    },
    /// The peer cancelled its page.
    PageStop {
        peer_id: String,
    },
    /// A device we paged was found and silenced.
    PageFound {
        peer_id: String,
        name: String,
    },
}

/// Receives node events on a background thread.
#[uniffi::export(with_foreign)]
pub trait NodeListener: Send + Sync {
    fn on_event(&self, event: NodeEventFfi);
}

/// Asked to confirm a six-digit code during code pairing. Blocking: return
/// once the user has answered.
#[uniffi::export(with_foreign)]
pub trait CodeConfirmer: Send + Sync {
    fn confirm(&self, code: String, peer_name: String) -> bool;
}

#[derive(uniffi::Object)]
pub struct Continuum {
    runtime: Runtime,
    identity: Mutex<Arc<DeviceIdentity>>,
    device: DeviceInfo,
    peers: Arc<Mutex<Vec<TrustedPeer>>>,
    node: Mutex<Option<NodeHandle>>,
}

impl std::fmt::Debug for Continuum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Continuum")
            .field("device_id", &self.identity().device_id())
            .finish_non_exhaustive()
    }
}

impl Continuum {
    fn peers(&self) -> std::sync::MutexGuard<'_, Vec<TrustedPeer>> {
        self.peers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn node(&self) -> Option<NodeHandle> {
        self.node
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn identity(&self) -> Arc<DeviceIdentity> {
        self.identity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn find_peer(&self, id: &str) -> Result<DeviceId, ContinuumError> {
        parse_device_id(id, &self.peers()).ok_or_else(|| invalid("unknown device"))
    }

    fn send_page(&self, peer_id: &str, action: PageAction) -> Result<(), ContinuumError> {
        let peer = self.find_peer(peer_id)?;
        let node = self.node().ok_or(ContinuumError::NotRunning)?;
        self.runtime
            .block_on(node.send_feature(peer, page::CAPABILITY, page::encode(action)))
            .map_err(|err| ContinuumError::Network {
                detail: err.to_string(),
            })
    }

    fn save_peer(&self, peer: TrustedPeer) -> PairedDevice {
        let info = PairedDevice::from(&peer);
        let snapshot = {
            let mut peers = self.peers();
            peers.retain(|p| p.device_id() != peer.device_id());
            peers.push(peer);
            peers.clone()
        };
        if let Some(node) = self.node() {
            self.runtime
                .spawn(async move { node.set_peers(snapshot).await });
        }
        info
    }

    fn spawn_on_node<F, Fut>(&self, f: F)
    where
        F: FnOnce(NodeHandle) -> Fut,
        Fut: std::future::Future<Output = Result<(), continuum_node::NodeError>> + Send + 'static,
    {
        if let Some(node) = self.node() {
            self.runtime.spawn(f(node));
        }
    }
}

fn parse_device_id(id: &str, peers: &[TrustedPeer]) -> Option<DeviceId> {
    peers
        .iter()
        .map(TrustedPeer::device_id)
        .find(|d| d.to_string() == id)
}

fn name_of(peers: &Mutex<Vec<TrustedPeer>>, id: &DeviceId) -> String {
    peers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|p| &p.device_id() == id)
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

/// Translates a node event, keeping the FFI's copy of the trust store in sync.
fn event_to_ffi(event: NodeEvent, peers: &Mutex<Vec<TrustedPeer>>) -> Option<NodeEventFfi> {
    Some(match event {
        NodeEvent::Advertise(ad) => NodeEventFfi::Advertise {
            advertisement: AdvertisementFfi {
                ble_service_uuid: BLE_SERVICE_UUID.to_owned(),
                beacon: ad.beacon.to_vec(),
                service_name: ad.record.name,
                service_type: ad
                    .record
                    .service_type
                    .trim_end_matches('.')
                    .trim_end_matches(".local")
                    .to_owned(),
                port: ad.record.port,
                txt: ad.record.txt.into_iter().collect(),
            },
        },
        NodeEvent::Peer(status) => NodeEventFfi::Peer {
            status: status.into(),
        },
        NodeEvent::SessionUp { peer, hello, .. } => NodeEventFfi::SessionUp {
            peer_id: peer.to_string(),
            name: hello.device.map(|d| d.name).unwrap_or_default(),
        },
        NodeEvent::SessionDown { peer, reason } => NodeEventFfi::SessionDown {
            peer_id: peer.to_string(),
            reason,
        },
        NodeEvent::PairingCandidate { addr, present, .. } => NodeEventFfi::PairingCandidate {
            addr: addr.map(|a| a.to_string()),
            present,
        },
        NodeEvent::PeerUpdated(updated) => {
            let mut peers = peers.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(stored) = peers
                .iter_mut()
                .find(|p| p.device_id() == updated.device_id())
            {
                *stored = updated;
            }
            NodeEventFfi::TrustChanged
        }
        NodeEvent::Unpaired { peer } => {
            let name = name_of(peers, &peer);
            peers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|p| p.device_id() != peer);
            NodeEventFfi::Unpaired {
                peer_id: peer.to_string(),
                name,
            }
        }
        NodeEvent::Feature {
            peer,
            capability,
            body,
            ..
        } if capability == page::CAPABILITY => {
            let peer_id = peer.to_string();
            match page::decode(&body)? {
                PageAction::Start { duration_s } => NodeEventFfi::PageStart {
                    name: name_of(peers, &peer),
                    peer_id,
                    duration_s,
                },
                PageAction::Stop => NodeEventFfi::PageStop { peer_id },
                PageAction::Found => NodeEventFfi::PageFound {
                    name: name_of(peers, &peer),
                    peer_id,
                },
            }
        }
        NodeEvent::Presence(_) | NodeEvent::Feature { .. } => return None,
    })
}

#[uniffi::export]
impl Continuum {
    /// Loads (or, if `identity_secret` is `None`, creates) a device.
    /// Persist [`Self::identity_secret`] and [`Self::trust_store`] in the
    /// Android Keystore-protected storage.
    ///
    /// # Errors
    /// Fails if the stored data is corrupt.
    #[uniffi::constructor]
    pub fn new(
        identity_secret: Option<Vec<u8>>,
        trust_store: Option<Vec<u8>>,
        name: String,
        kind: DeviceKindFfi,
    ) -> Result<Arc<Self>, ContinuumError> {
        let identity = match identity_secret {
            Some(bytes) => DeviceIdentity::from_secret_bytes(&bytes).map_err(invalid)?,
            None => DeviceIdentity::generate(&mut OsEntropy),
        };
        let peers = match trust_store {
            Some(bytes) => decode_peers(&bytes).map_err(invalid)?,
            None => Vec::new(),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("continuum")
            .enable_all()
            .build()
            .map_err(invalid)?;
        Ok(Arc::new(Self {
            runtime,
            identity: Mutex::new(Arc::new(identity)),
            device: DeviceInfo {
                name: continuum_core::pairing::sanitize_name(&name),
                kind: DeviceKind::from(kind).into(),
                protocol_version: PROTOCOL_VERSION,
            },
            peers: Arc::new(Mutex::new(peers)),
            node: Mutex::new(None),
        }))
    }

    /// Serialised identity (secret). Save again after [`Self::unpair`],
    /// which rotates the presence key.
    pub fn identity_secret(&self) -> Vec<u8> {
        self.identity().to_secret_bytes().to_vec()
    }

    /// Serialised trust store (contains secrets). Save after pairing changes.
    pub fn trust_store(&self) -> Vec<u8> {
        encode_peers(self.peers().iter()).to_vec()
    }

    pub fn device_id(&self) -> String {
        self.identity().device_id().to_string()
    }

    pub fn paired_devices(&self) -> Vec<PairedDevice> {
        self.peers().iter().map(PairedDevice::from).collect()
    }

    /// Starts the node and returns the TCP port it listens on. Events go to
    /// `listener`; the first is always `Advertise`. `rotation_secs`
    /// overrides the identifier rotation interval (for testing).
    ///
    /// # Errors
    /// Fails if the listener cannot be bound.
    pub fn start(
        &self,
        listener: Arc<dyn NodeListener>,
        rotation_secs: Option<u32>,
    ) -> Result<u16, ContinuumError> {
        if let Some(node) = self.node() {
            return Ok(node.port());
        }
        let mut config =
            NodeConfig::new(self.identity(), self.peers().clone(), self.device.clone());
        config.capabilities = vec![page::capability_v1()];
        if let Some(secs) = rotation_secs {
            config.rotation = Duration::from_secs(u64::from(secs.max(10)));
        }
        let (node, mut events) = self
            .runtime
            .block_on(continuum_node::start(config))
            .map_err(|err| ContinuumError::Network {
                detail: err.to_string(),
            })?;
        let port = node.port();
        let peers = self.peers.clone();
        self.runtime.spawn(async move {
            while let Some(event) = events.recv().await {
                if let Some(event) = event_to_ffi(event, &peers) {
                    let listener = listener.clone();
                    // Foreign callbacks may block; keep them off the reactor.
                    let _ = tokio::task::spawn_blocking(move || listener.on_event(event)).await;
                }
            }
        });
        *self.node.lock().unwrap_or_else(PoisonError::into_inner) = Some(node);
        Ok(port)
    }

    /// Says goodbye to all peers and stops the node.
    pub fn stop(&self) {
        let node = self
            .node
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(node) = node {
            let _ = self.runtime.block_on(node.shutdown());
        }
    }

    /// Current status of every paired device.
    pub fn statuses(&self) -> Vec<PeerStatusFfi> {
        match self.node() {
            Some(node) => self
                .runtime
                .block_on(node.snapshot())
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Reports a resolved `_continuum._tcp` service from NsdManager.
    ///
    /// # Errors
    /// Fails if `host` is not an IP address.
    ///
    /// `host` may carry a `%zone` suffix; pass the interface index as
    /// `scope_id` (0 if none) so link-local IPv6 addresses stay usable.
    pub fn observe_lan(
        &self,
        name: String,
        txt: HashMap<String, String>,
        host: String,
        port: u16,
        scope_id: u32,
    ) -> Result<(), ContinuumError> {
        let host = host.trim_start_matches('/');
        let host = host.split_once('%').map_or(host, |(ip, _)| ip);
        let ip: IpAddr = host.parse().map_err(invalid)?;
        let addr = match ip {
            IpAddr::V6(v6) if scope_id != 0 => {
                SocketAddr::V6(SocketAddrV6::new(v6, port, 0, scope_id))
            }
            _ => SocketAddr::new(ip, port),
        };
        let txt: Vec<(String, String)> = txt.into_iter().collect();
        self.spawn_on_node(move |node| async move { node.observe_lan(name, txt, addr).await });
        Ok(())
    }

    /// Reports that a `_continuum._tcp` service was lost.
    pub fn withdraw_lan(&self, name: String) {
        self.spawn_on_node(move |node| async move { node.withdraw_lan(name).await });
    }

    /// Reports service data seen under the Continuum BLE UUID.
    pub fn observe_ble(&self, service_data: Vec<u8>, rssi: i16) {
        self.spawn_on_node(move |node| async move { node.observe_ble(service_data, rssi).await });
    }

    /// Pairs using the text from a Continuum QR code. Blocks until done.
    ///
    /// # Errors
    /// Fails if the invite is invalid or pairing fails.
    pub fn join_invite(&self, invite: String) -> Result<PairedDevice, ContinuumError> {
        let peer = self
            .runtime
            .block_on(join_invite(
                &self.identity(),
                self.device.clone(),
                &invite,
                &SystemClock,
            ))
            .map_err(|err| ContinuumError::Pairing {
                detail: err.to_string(),
            })?;
        Ok(self.save_peer(peer))
    }

    /// Pairs by code comparison with a device in pairing mode at `addr`
    /// (`ip:port`). Blocks until done.
    ///
    /// # Errors
    /// Fails if the address is invalid, a user rejects, or pairing fails.
    pub fn join_with_code(
        &self,
        addr: String,
        confirmer: Arc<dyn CodeConfirmer>,
    ) -> Result<PairedDevice, ContinuumError> {
        let addr: SocketAddr = addr.parse().map_err(invalid)?;
        let confirm = move |sas: continuum_core::session::Sas, peer: TrustedPeer| async move {
            tokio::task::spawn_blocking(move || confirmer.confirm(sas.to_string(), peer.name))
                .await
                .unwrap_or(false)
        };
        let peer = self
            .runtime
            .block_on(join_with_code(
                &self.identity(),
                self.device.clone(),
                &[addr],
                confirm,
                &SystemClock,
            ))
            .map_err(|err| ContinuumError::Pairing {
                detail: err.to_string(),
            })?;
        Ok(self.save_peer(peer))
    }

    /// Forgets a paired device and tells it to forget us, then rotates our
    /// presence key so it can no longer recognise our beacons. Persist both
    /// [`Self::identity_secret`] and [`Self::trust_store`] afterwards.
    ///
    /// # Errors
    /// Fails if no paired device has that id.
    pub fn unpair(&self, id: String) -> Result<(), ContinuumError> {
        let peer = self.find_peer(&id)?;
        self.peers().retain(|p| p.device_id() != peer);
        let rotated = Arc::new(self.identity().with_new_presence_key(&mut OsEntropy));
        *self.identity.lock().unwrap_or_else(PoisonError::into_inner) = rotated.clone();
        if let Some(node) = self.node() {
            self.runtime.block_on(async {
                let _ = node.unpair(peer).await;
                let _ = node.set_identity(rotated).await;
            });
        }
        Ok(())
    }

    /// Makes a connected peer ring, even on silent.
    ///
    /// # Errors
    /// Fails if the peer is unknown, not connected, or can't be paged.
    pub fn page(&self, peer_id: String, duration_s: u32) -> Result<(), ContinuumError> {
        self.send_page(&peer_id, PageAction::Start { duration_s })
    }

    /// Cancels a page we started.
    ///
    /// # Errors
    /// Fails if the peer is unknown or not connected.
    pub fn stop_page(&self, peer_id: String) -> Result<(), ContinuumError> {
        self.send_page(&peer_id, PageAction::Stop)
    }

    /// Tells the pager that the user found this device.
    ///
    /// # Errors
    /// Fails if the peer is unknown or not connected.
    pub fn page_found(&self, peer_id: String) -> Result<(), ContinuumError> {
        self.send_page(&peer_id, PageAction::Found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_trust_round_trip() {
        let device = Continuum::new(None, None, "Pixel".into(), DeviceKindFfi::Phone).unwrap();
        let again = Continuum::new(
            Some(device.identity_secret()),
            Some(device.trust_store()),
            "Pixel".into(),
            DeviceKindFfi::Phone,
        )
        .unwrap();
        assert_eq!(device.device_id(), again.device_id());
        assert!(again.paired_devices().is_empty());
    }

    #[test]
    fn rejects_corrupt_identity() {
        assert!(
            Continuum::new(Some(vec![1, 2, 3]), None, "x".into(), DeviceKindFfi::Phone).is_err()
        );
    }

    struct Collect(Mutex<Vec<NodeEventFfi>>);

    impl NodeListener for Collect {
        fn on_event(&self, event: NodeEventFfi) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn start_emits_advertisement() {
        let device = Continuum::new(None, None, "Pixel".into(), DeviceKindFfi::Phone).unwrap();
        let listener = Arc::new(Collect(Mutex::new(Vec::new())));
        let port = device.start(listener.clone(), None).unwrap();
        assert_ne!(port, 0);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let events = listener.0.lock().unwrap().clone();
        let Some(NodeEventFfi::Advertise { advertisement }) = events.first() else {
            panic!("expected advertisement, got {events:?}");
        };
        assert_eq!(advertisement.service_type, "_continuum._tcp");
        assert_eq!(advertisement.port, port);
        assert_eq!(advertisement.beacon.len(), 13);
        device.stop();
    }
}
