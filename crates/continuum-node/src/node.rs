//! The node event loop.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use continuum_core::capabilities::{Negotiated, capability, negotiate};
use continuum_core::clock::{Clock, SystemClock, Timestamp};
use continuum_core::entropy::OsEntropy;
use continuum_core::identity::{DeviceId, DeviceIdentity};
use continuum_core::presence::{
    PresenceEvent, PresenceRegistry, ProximityBand, RegistryConfig, Transport,
};
use continuum_core::proto::ResolvableId;
use continuum_core::proto::beacon::{BEACON_LEN, Beacon, BeaconFlags};
use continuum_core::proto::features::trust::v1 as trust;
use continuum_core::proto::v1::{
    Capability, CapabilityManifest, DeviceInfo, FeatureMessage, Goodbye, Hello, envelope, goodbye,
};
use continuum_core::rid::{PresenceKey, RotatingId};
use continuum_core::trust::TrustedPeer;
use continuum_net::{
    Intent, LinkClosed, SessionLink, accept_session, connect_session, read_preamble, spawn_link,
};
use prost::Message;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::records::{
    ServiceRecord, ble_observation, continuum_id, continuum_record, lan_observation,
};
use crate::{NodeError, connect_any, rank_addrs};

/// The higher-id side waits this long for the lower-id side to connect
/// before trying itself, which avoids duplicate sessions.
const CONNECT_GRACE: Duration = Duration::from_secs(10);
const RETRY_AFTER: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// RSSI-only status updates: at most this often per peer...
const RSSI_UPDATE_EVERY: Duration = Duration::from_millis(500);
/// ...and only if the smoothed RSSI moved at least this much (dB).
const RSSI_UPDATE_DELTA: f32 = 1.0;

/// Capabilities the node implements itself.
const TRUST: &str = "trust";

fn built_in_capabilities() -> Vec<Capability> {
    vec![capability("presence", 1, 1), capability(TRUST, 1, 1)]
}

#[derive(Debug)]
pub struct NodeConfig {
    pub identity: Arc<DeviceIdentity>,
    pub peers: Vec<TrustedPeer>,
    pub device: DeviceInfo,
    /// Features the platform implements, on top of the built-in ones.
    pub capabilities: Vec<Capability>,
    /// Where to accept sessions; port 0 picks a free port.
    pub listen: SocketAddr,
    pub keepalive: Duration,
    pub rotation: Duration,
    pub registry: RegistryConfig,
}

impl NodeConfig {
    pub fn new(identity: Arc<DeviceIdentity>, peers: Vec<TrustedPeer>, device: DeviceInfo) -> Self {
        Self {
            identity,
            peers,
            device,
            capabilities: Vec::new(),
            listen: SocketAddr::from(([0u16; 8], 0)),
            keepalive: Duration::from_secs(10),
            rotation: RotatingId::DEFAULT_INTERVAL,
            registry: RegistryConfig::default(),
        }
    }
}

/// What the platform should currently advertise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advertisement {
    pub id: ResolvableId,
    /// BLE service data for [`continuum_core::proto::BLE_SERVICE_UUID`].
    pub beacon: [u8; BEACON_LEN],
    /// DNS-SD record to publish.
    pub record: ServiceRecord,
}

/// Everything known about one paired peer.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerStatus {
    pub id: DeviceId,
    pub name: String,
    pub nearby: bool,
    pub reachable: bool,
    pub connected: bool,
    pub band: Option<ProximityBand>,
    pub rssi: Option<f32>,
    pub addrs: Vec<SocketAddr>,
    /// Features usable with this peer right now (empty when disconnected).
    pub capabilities: Negotiated,
}

#[derive(Debug, Clone)]
pub enum NodeEvent {
    /// Publish this instead of whatever was advertised before.
    Advertise(Advertisement),
    /// A paired peer's status changed.
    Peer(PeerStatus),
    /// Raw presence change, for logging.
    Presence(PresenceEvent),
    SessionUp {
        peer: DeviceId,
        hello: Hello,
        capabilities: Negotiated,
    },
    SessionDown {
        peer: DeviceId,
        reason: String,
    },
    /// A stored peer changed (e.g. it rotated its presence key); persist it.
    PeerUpdated(TrustedPeer),
    /// The peer unpaired us; remove it from persistent storage.
    Unpaired {
        peer: DeviceId,
    },
    /// A device in pairing mode was seen (or is gone).
    PairingCandidate {
        id: ResolvableId,
        transport: Transport,
        addr: Option<SocketAddr>,
        present: bool,
    },
    /// A message for a negotiated feature.
    Feature {
        peer: DeviceId,
        capability: String,
        version: u32,
        body: Vec<u8>,
    },
}

enum Command {
    Lan {
        name: String,
        txt: Vec<(String, String)>,
        addr: SocketAddr,
    },
    LanRemoved {
        name: String,
    },
    Ble {
        data: Vec<u8>,
        rssi: i16,
    },
    SetPeers(Vec<TrustedPeer>),
    Unpair {
        peer: DeviceId,
        reply: oneshot::Sender<Result<(), NodeError>>,
    },
    SetIdentity(Arc<DeviceIdentity>),
    Feature {
        peer: DeviceId,
        capability: String,
        body: Vec<u8>,
        reply: oneshot::Sender<Result<(), NodeError>>,
    },
    Snapshot(oneshot::Sender<Vec<PeerStatus>>),
    Shutdown(oneshot::Sender<()>),
}

/// Cheap, cloneable handle to a running node.
#[derive(Debug, Clone)]
pub struct NodeHandle {
    tx: mpsc::Sender<Command>,
    port: u16,
}

impl NodeHandle {
    /// The TCP port the node accepts sessions on.
    pub fn port(&self) -> u16 {
        self.port
    }

    async fn command(&self, command: Command) -> Result<(), NodeError> {
        self.tx.send(command).await.map_err(|_| NodeError::Stopped)
    }

    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, NodeError>>) -> Command,
    ) -> Result<T, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.command(make(tx)).await?;
        rx.await.map_err(|_| NodeError::Stopped)?
    }

    /// Reports a resolved `_continuum._tcp` service. Non-Continuum or
    /// untrusted records are ignored.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn observe_lan(
        &self,
        name: String,
        txt: Vec<(String, String)>,
        addr: SocketAddr,
    ) -> Result<(), NodeError> {
        self.command(Command::Lan { name, txt, addr }).await
    }

    /// Reports that a `_continuum._tcp` service went away.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn withdraw_lan(&self, name: String) -> Result<(), NodeError> {
        self.command(Command::LanRemoved { name }).await
    }

    /// Reports BLE service data seen under the Continuum UUID.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn observe_ble(&self, data: Vec<u8>, rssi: i16) -> Result<(), NodeError> {
        self.command(Command::Ble { data, rssi }).await
    }

    /// Replaces the trust store, e.g. after pairing. Sessions with peers
    /// that are no longer listed are closed.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn set_peers(&self, peers: Vec<TrustedPeer>) -> Result<(), NodeError> {
        self.command(Command::SetPeers(peers)).await
    }

    /// Forgets `peer` and tells it so, if connected, so it forgets us too.
    /// The caller should also rotate the presence key with
    /// [`Self::set_identity`], so the removed peer can no longer track us.
    ///
    /// # Errors
    /// Fails if `peer` isn't paired or the node has stopped.
    pub async fn unpair(&self, peer: DeviceId) -> Result<(), NodeError> {
        self.request(|reply| Command::Unpair { peer, reply }).await
    }

    /// Switches to an identity with a new presence key. Connected peers are
    /// sent the new key at once; others get it in the next session's Hello.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn set_identity(&self, identity: Arc<DeviceIdentity>) -> Result<(), NodeError> {
        self.command(Command::SetIdentity(identity)).await
    }

    /// Sends a feature message to a connected peer.
    ///
    /// # Errors
    /// Fails if the peer isn't connected or didn't negotiate `capability`.
    pub async fn send_feature(
        &self,
        peer: DeviceId,
        capability: &str,
        body: Vec<u8>,
    ) -> Result<(), NodeError> {
        let capability = capability.to_owned();
        self.request(|reply| Command::Feature {
            peer,
            capability,
            body,
            reply,
        })
        .await
    }

    /// Current status of every paired peer.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn snapshot(&self) -> Result<Vec<PeerStatus>, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.command(Command::Snapshot(tx)).await?;
        rx.await.map_err(|_| NodeError::Stopped)
    }

    /// Says goodbye to every peer and stops.
    ///
    /// # Errors
    /// Fails if the node has already stopped.
    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let (tx, rx) = oneshot::channel();
        self.command(Command::Shutdown(tx)).await?;
        rx.await.map_err(|_| NodeError::Stopped)
    }
}

/// Binds the listener and starts the node in the background.
///
/// The first event is always [`NodeEvent::Advertise`].
///
/// # Errors
/// Fails if the listener cannot be bound.
pub async fn start(
    config: NodeConfig,
) -> Result<(NodeHandle, mpsc::Receiver<NodeEvent>), NodeError> {
    let listener = TcpListener::bind(config.listen).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = mpsc::channel(64);
    let (events_tx, events) = mpsc::channel(256);
    tokio::spawn(Node::new(config, port, events_tx).run(listener, rx));
    Ok((NodeHandle { tx, port }, events))
}

enum Internal {
    Up {
        link: Box<SessionLink>,
        initiated: bool,
    },
    Down {
        peer: DeviceId,
        reason: LinkClosed,
    },
    ConnectFailed {
        peer: DeviceId,
    },
    Message {
        peer: DeviceId,
        body: envelope::Body,
    },
}

/// TXT entries and addresses of one resolved DNS-SD service.
type LanRecord = (Vec<(String, String)>, HashSet<SocketAddr>);

struct Live {
    sender: mpsc::Sender<envelope::Body>,
    initiated: bool,
    capabilities: Negotiated,
}

struct Node {
    identity: Arc<DeviceIdentity>,
    our_id: DeviceId,
    peers: Vec<TrustedPeer>,
    device: DeviceInfo,
    capabilities: Vec<Capability>,
    keepalive: Duration,
    rotation: Duration,
    port: u16,
    clock: SystemClock,
    rotating: RotatingId,
    registry: PresenceRegistry,
    sessions: HashMap<DeviceId, Live>,
    connecting: HashSet<DeviceId>,
    last_attempt: HashMap<DeviceId, Timestamp>,
    reachable_since: HashMap<DeviceId, Timestamp>,
    /// Last RSSI reported per peer, for throttling radar updates.
    last_rssi: HashMap<DeviceId, (Timestamp, f32)>,
    /// Every live LAN record, trusted or not, so a record seen before its
    /// owner was paired is picked up as soon as the trust store changes
    /// (mDNS stacks only report a record once).
    lan_records: HashMap<String, LanRecord>,
    events: mpsc::Sender<NodeEvent>,
    internal_tx: mpsc::Sender<Internal>,
    internal_rx: Option<mpsc::Receiver<Internal>>,
}

fn feature(capability: &str, version: u32, body: Vec<u8>) -> envelope::Body {
    envelope::Body::Feature(FeatureMessage {
        capability: capability.to_owned(),
        version,
        body,
    })
}

impl Node {
    fn new(config: NodeConfig, port: u16, events: mpsc::Sender<NodeEvent>) -> Self {
        let mut registry = PresenceRegistry::new(config.registry);
        registry.set_trusted_peers(&config.peers);
        let (internal_tx, internal_rx) = mpsc::channel(64);
        let mut capabilities = built_in_capabilities();
        capabilities.extend(config.capabilities);
        Self {
            our_id: config.identity.device_id(),
            rotating: RotatingId::new(config.identity.presence_key().clone(), config.rotation),
            identity: config.identity,
            peers: config.peers,
            device: config.device,
            capabilities,
            keepalive: config.keepalive,
            rotation: config.rotation,
            port,
            clock: SystemClock,
            registry,
            sessions: HashMap::new(),
            connecting: HashSet::new(),
            last_attempt: HashMap::new(),
            reachable_since: HashMap::new(),
            last_rssi: HashMap::new(),
            lan_records: HashMap::new(),
            events,
            internal_tx,
            internal_rx: Some(internal_rx),
        }
    }

    fn hello(&self) -> Hello {
        Hello {
            device: Some(self.device.clone()),
            manifest: Some(CapabilityManifest {
                capabilities: self.capabilities.clone(),
            }),
            presence_key: self.identity.presence_key().as_bytes().to_vec(),
        }
    }

    async fn emit(&self, event: NodeEvent) {
        let _ = self.events.send(event).await;
    }

    fn advertisement(&self, id: ResolvableId) -> Advertisement {
        let flags = BeaconFlags::LAN;
        Advertisement {
            id,
            beacon: Beacon { flags, id }.encode(),
            record: continuum_record(id, self.port, flags),
        }
    }

    fn status(&self, peer: &TrustedPeer) -> PeerStatus {
        let id = peer.device_id();
        let presence = self.registry.presence(&id);
        let ble = presence.and_then(|p| p.ble.as_ref());
        let lan = presence.and_then(|p| p.lan.as_ref());
        let live = self.sessions.get(&id);
        PeerStatus {
            id,
            name: peer.name.clone(),
            nearby: ble.is_some(),
            reachable: lan.is_some(),
            connected: live.is_some(),
            band: ble.and_then(|b| b.band),
            rssi: ble.and_then(|b| b.rssi),
            addrs: lan.map(|l| rank_addrs(l.addrs())).unwrap_or_default(),
            capabilities: live.map(|l| l.capabilities.clone()).unwrap_or_default(),
        }
    }

    async fn emit_status(&self, id: &DeviceId) {
        if let Some(peer) = self.peers.iter().find(|p| &p.device_id() == id) {
            self.emit(NodeEvent::Peer(self.status(peer))).await;
        }
    }

    async fn handle_presence(&mut self, events: Vec<PresenceEvent>) {
        let mut changed = Vec::new();
        for event in events {
            match event {
                PresenceEvent::Appeared { peer, .. }
                | PresenceEvent::ProximityChanged { peer, .. }
                | PresenceEvent::AddressesChanged { peer } => changed.push(peer),
                PresenceEvent::Lost { peer, transport } => {
                    if transport == Transport::Lan {
                        self.reachable_since.remove(&peer);
                    }
                    changed.push(peer);
                }
                PresenceEvent::PairingCandidateAppeared { id, transport }
                | PresenceEvent::PairingCandidateLost { id, transport } => {
                    let present = matches!(event, PresenceEvent::PairingCandidateAppeared { .. });
                    let addr = self
                        .registry
                        .pairing_candidates()
                        .find(|c| c.id == id && c.transport == transport)
                        .and_then(|c| c.addr);
                    self.emit(NodeEvent::PairingCandidate {
                        id,
                        transport,
                        addr,
                        present,
                    })
                    .await;
                }
            }
            self.emit(NodeEvent::Presence(event)).await;
        }
        changed.dedup();
        for peer in changed {
            self.emit_status(&peer).await;
        }
    }

    /// Re-checks every known LAN record against the current trust store.
    async fn replay_lan_records(&mut self, now: Timestamp) {
        let observations: Vec<_> = self
            .lan_records
            .iter()
            .flat_map(|(name, (txt, addrs))| {
                addrs.iter().filter_map(move |addr| lan_observation(name, txt, *addr, now))
            })
            .collect();
        for obs in observations {
            let events = self.registry.ingest(&obs);
            self.handle_presence(events).await;
        }
    }

    /// Streams RSSI changes (for radars) without flooding the platform.
    async fn maybe_report_rssi(&mut self, id: &ResolvableId, now: Timestamp) {
        let Some(peer) = self.registry.resolve_peer(id) else {
            return;
        };
        let Some(rssi) = self
            .registry
            .presence(&peer)
            .and_then(|p| p.ble.as_ref())
            .and_then(|ble| ble.rssi)
        else {
            return;
        };
        let due = self.last_rssi.get(&peer).is_none_or(|(at, last)| {
            now.saturating_since(*at) >= RSSI_UPDATE_EVERY
                && (rssi - last).abs() >= RSSI_UPDATE_DELTA
        });
        if due {
            self.last_rssi.insert(peer, (now, rssi));
            self.emit_status(&peer).await;
        }
    }

    fn maybe_connect(&mut self, now: Timestamp) {
        let candidates: Vec<(DeviceId, Vec<SocketAddr>)> = self
            .registry
            .peers()
            .filter_map(|(id, p)| Some((*id, rank_addrs(p.lan.as_ref()?.addrs()))))
            .collect();
        for (peer, addrs) in candidates {
            if self.sessions.contains_key(&peer) || self.connecting.contains(&peer) {
                continue;
            }
            let since = *self.reachable_since.entry(peer).or_insert(now);
            let our_turn = self.our_id < peer || now.saturating_since(since) >= CONNECT_GRACE;
            let backed_off = self
                .last_attempt
                .get(&peer)
                .is_some_and(|t| now.saturating_since(*t) < RETRY_AFTER);
            let Some(trusted) = self.peers.iter().find(|p| p.device_id() == peer) else {
                continue;
            };
            if our_turn && !backed_off {
                self.connecting.insert(peer);
                self.last_attempt.insert(peer, now);
                tokio::spawn(outgoing(
                    addrs,
                    self.identity.clone(),
                    trusted.clone(),
                    self.hello(),
                    self.internal_tx.clone(),
                    self.keepalive,
                ));
            }
        }
    }

    /// Adopts a presence key a peer announced, if it changed.
    async fn update_presence_key(&mut self, peer: DeviceId, key: &[u8]) {
        let Ok(key) = <[u8; 32]>::try_from(key) else {
            return;
        };
        let key = PresenceKey::from_bytes(key);
        let Some(stored) = self.peers.iter_mut().find(|p| p.device_id() == peer) else {
            return;
        };
        if stored.presence_key == key {
            return;
        }
        stored.presence_key = key;
        let updated = stored.clone();
        let events = self.registry.set_trusted_peers(&self.peers);
        self.handle_presence(events).await;
        tracing::info!(peer = %peer, "peer rotated its presence key");
        self.emit(NodeEvent::PeerUpdated(updated)).await;
    }

    async fn forget(&mut self, peer: DeviceId) {
        self.peers.retain(|p| p.device_id() != peer);
        self.sessions.remove(&peer);
        let events = self.registry.set_trusted_peers(&self.peers);
        self.handle_presence(events).await;
    }

    async fn handle_internal(&mut self, event: Internal) {
        match event {
            Internal::Up { link, initiated } => {
                let peer = link.peer.device_id();
                self.connecting.remove(&peer);
                // Keep the session started by the lower device id.
                let preferred = initiated == (self.our_id < peer);
                if let Some(existing) = self.sessions.get(&peer)
                    && (existing.initiated == initiated || !preferred)
                {
                    return;
                }
                let hello = link.hello.clone();
                let theirs = hello
                    .manifest
                    .as_ref()
                    .map(|m| m.capabilities.as_slice())
                    .unwrap_or_default();
                let capabilities = negotiate(&self.capabilities, theirs);
                self.sessions.insert(
                    peer,
                    Live {
                        sender: link.sender(),
                        initiated,
                        capabilities: capabilities.clone(),
                    },
                );
                tokio::spawn(watch(*link, self.internal_tx.clone()));
                self.update_presence_key(peer, &hello.presence_key).await;
                self.emit(NodeEvent::SessionUp {
                    peer,
                    hello,
                    capabilities,
                })
                .await;
                self.emit_status(&peer).await;
            }
            Internal::Down { peer, reason } => {
                // Ignore the end of a duplicate we already replaced.
                if !self
                    .sessions
                    .get(&peer)
                    .is_some_and(|live| live.sender.is_closed())
                {
                    return;
                }
                self.sessions.remove(&peer);
                let unpaired = matches!(
                    reason,
                    LinkClosed::Goodbye(code) if code == i32::from(goodbye::Reason::Unpaired)
                );
                self.emit(NodeEvent::SessionDown {
                    peer,
                    reason: format!("{reason:?}"),
                })
                .await;
                if unpaired && self.peers.iter().any(|p| p.device_id() == peer) {
                    self.forget(peer).await;
                    self.emit(NodeEvent::Unpaired { peer }).await;
                } else {
                    self.emit_status(&peer).await;
                }
            }
            Internal::ConnectFailed { peer } => {
                self.connecting.remove(&peer);
            }
            Internal::Message { peer, body } => {
                let envelope::Body::Feature(message) = body else {
                    return;
                };
                let Some(&version) = self
                    .sessions
                    .get(&peer)
                    .and_then(|live| live.capabilities.get(&message.capability))
                else {
                    tracing::debug!(capability = %message.capability, "dropping un-negotiated feature message");
                    return;
                };
                if message.capability == TRUST {
                    if let Ok(trust::Trust {
                        action: Some(trust::trust::Action::PresenceKey(key)),
                    }) = trust::Trust::decode(message.body.as_slice())
                    {
                        self.update_presence_key(peer, &key.key).await;
                    }
                    return;
                }
                self.emit(NodeEvent::Feature {
                    peer,
                    capability: message.capability,
                    version,
                    body: message.body,
                })
                .await;
            }
        }
    }

    #[allow(clippy::too_many_lines)] // One arm per command.
    async fn handle_command(&mut self, command: Command) -> bool {
        let now = self.clock.now();
        match command {
            Command::Lan { name, txt, addr } => {
                if let Some(obs) = lan_observation(&name, &txt, addr, now) {
                    const MAX_RECORDS: usize = 256;
                    if self.lan_records.len() >= MAX_RECORDS && !self.lan_records.contains_key(&name) {
                        self.lan_records.clear();
                    }
                    let entry = self.lan_records.entry(name).or_default();
                    entry.0 = txt;
                    entry.1.insert(addr);
                    let events = self.registry.ingest(&obs);
                    self.handle_presence(events).await;
                }
            }
            Command::LanRemoved { name } => {
                self.lan_records.remove(&name);
                if let Some(id) = continuum_id(&name) {
                    let events = self.registry.withdraw(Transport::Lan, &id);
                    self.handle_presence(events).await;
                }
            }
            Command::Ble { data, rssi } => {
                if let Some(obs) = ble_observation(&data, rssi, now) {
                    let events = self.registry.ingest(&obs);
                    let changed = !events.is_empty();
                    self.handle_presence(events).await;
                    if !changed {
                        self.maybe_report_rssi(&obs.id, now).await;
                    }
                }
            }
            Command::SetPeers(peers) => {
                self.peers = peers;
                let trusted: HashSet<DeviceId> =
                    self.peers.iter().map(TrustedPeer::device_id).collect();
                self.sessions.retain(|id, _| trusted.contains(id));
                let events = self.registry.set_trusted_peers(&self.peers);
                self.handle_presence(events).await;
                self.replay_lan_records(now).await;
            }
            Command::Unpair { peer, reply } => {
                if !self.peers.iter().any(|p| p.device_id() == peer) {
                    let _ = reply.send(Err(NodeError::UnknownPeer));
                    return true;
                }
                if let Some(live) = self.sessions.get(&peer) {
                    let goodbye = Goodbye {
                        reason: goodbye::Reason::Unpaired.into(),
                    };
                    let _ = live.sender.send(envelope::Body::Goodbye(goodbye)).await;
                }
                self.forget(peer).await;
                let _ = reply.send(Ok(()));
            }
            Command::SetIdentity(identity) => {
                self.identity = identity;
                self.rotating =
                    RotatingId::new(self.identity.presence_key().clone(), self.rotation);
                let (id, _) = self.rotating.current(now, &mut OsEntropy);
                self.emit(NodeEvent::Advertise(self.advertisement(id)))
                    .await;
                let body = trust::Trust {
                    action: Some(trust::trust::Action::PresenceKey(trust::PresenceKey {
                        key: self.identity.presence_key().as_bytes().to_vec(),
                    })),
                }
                .encode_to_vec();
                for live in self.sessions.values() {
                    if let Some(&version) = live.capabilities.get(TRUST) {
                        let _ = live
                            .sender
                            .send(feature(TRUST, version, body.clone()))
                            .await;
                    }
                }
            }
            Command::Feature {
                peer,
                capability,
                body,
                reply,
            } => {
                let result = match self.sessions.get(&peer) {
                    None => Err(NodeError::NotConnected),
                    Some(live) => match live.capabilities.get(&capability) {
                        None => Err(NodeError::NotNegotiated(capability.clone())),
                        Some(&version) => live
                            .sender
                            .send(feature(&capability, version, body))
                            .await
                            .map_err(|_| NodeError::NotConnected),
                    },
                };
                let _ = reply.send(result);
            }
            Command::Snapshot(reply) => {
                let statuses = self.peers.iter().map(|p| self.status(p)).collect();
                let _ = reply.send(statuses);
            }
            Command::Shutdown(reply) => {
                self.sessions.clear();
                // Give links a moment to send their goodbyes.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let _ = reply.send(());
                return false;
            }
        }
        true
    }

    async fn run(mut self, listener: TcpListener, mut commands: mpsc::Receiver<Command>) {
        let mut internal = self.internal_rx.take().expect("run is called once");
        let (id, _) = self.rotating.current(self.clock.now(), &mut OsEntropy);
        self.emit(NodeEvent::Advertise(self.advertisement(id)))
            .await;
        let mut tick = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    if let Ok((stream, _)) = accepted {
                        tokio::spawn(incoming(
                            stream,
                            self.identity.clone(),
                            self.peers.clone(),
                            self.hello(),
                            self.internal_tx.clone(),
                            self.keepalive,
                        ));
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    if !self.handle_command(command).await {
                        return;
                    }
                }
                Some(event) = internal.recv() => self.handle_internal(event).await,
                _ = tick.tick() => {
                    let now = self.clock.now();
                    let events = self.registry.expire(now);
                    self.handle_presence(events).await;
                    let (id, rotated) = self.rotating.current(now, &mut OsEntropy);
                    if rotated {
                        self.emit(NodeEvent::Advertise(self.advertisement(id))).await;
                    }
                    self.maybe_connect(now);
                }
            }
        }
    }
}

async fn incoming(
    mut stream: TcpStream,
    identity: Arc<DeviceIdentity>,
    peers: Vec<TrustedPeer>,
    hello: Hello,
    internal: mpsc::Sender<Internal>,
    keepalive: Duration,
) {
    stream.set_nodelay(true).ok();
    let from = stream.peer_addr().ok();
    let Ok(Ok(intent)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_preamble(&mut stream)).await
    else {
        return;
    };
    if intent != Intent::Session {
        tracing::debug!(?from, ?intent, "ignoring non-session connection");
        return;
    }
    match tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        accept_session(stream, &identity, &peers, hello),
    )
    .await
    {
        Ok(Ok(session)) => {
            let link = Box::new(spawn_link(session, keepalive));
            let _ = internal
                .send(Internal::Up {
                    link,
                    initiated: false,
                })
                .await;
        }
        Ok(Err(err)) => tracing::debug!(?from, %err, "rejected session"),
        Err(_) => tracing::debug!(?from, "session handshake timed out"),
    }
}

async fn outgoing(
    addrs: Vec<SocketAddr>,
    identity: Arc<DeviceIdentity>,
    peer: TrustedPeer,
    hello: Hello,
    internal: mpsc::Sender<Internal>,
    keepalive: Duration,
) {
    let id = peer.device_id();
    let result = async {
        let stream = connect_any(&addrs).await?;
        let session = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            connect_session(stream, &identity, &peer, hello),
        )
        .await
        .map_err(|_| NodeError::TimedOut)??;
        Ok::<_, NodeError>(spawn_link(session, keepalive))
    }
    .await;
    let event = match result {
        Ok(link) => Internal::Up {
            link: Box::new(link),
            initiated: true,
        },
        Err(err) => {
            tracing::debug!(%err, "connect failed");
            Internal::ConnectFailed { peer: id }
        }
    };
    let _ = internal.send(event).await;
}

async fn watch(link: SessionLink, internal: mpsc::Sender<Internal>) {
    let (peer, _, mut incoming, closed) = link.into_receivers();
    let peer = peer.device_id();
    let forward = internal.clone();
    tokio::spawn(async move {
        while let Some(body) = incoming.recv().await {
            let _ = forward.send(Internal::Message { peer, body }).await;
        }
    });
    let reason = closed
        .await
        .unwrap_or(LinkClosed::Error(continuum_net::NetError::Protocol(
            "dropped",
        )));
    let _ = internal.send(Internal::Down { peer, reason }).await;
}

#[cfg(test)]
mod tests {
    use continuum_core::clock::Timestamp;
    use continuum_core::entropy::DeterministicEntropy;

    use super::*;

    fn identity(seed: u64) -> Arc<DeviceIdentity> {
        Arc::new(DeviceIdentity::generate(&mut DeterministicEntropy::new(
            seed,
        )))
    }

    fn trusted(identity: &DeviceIdentity, name: &str) -> TrustedPeer {
        TrustedPeer {
            identity: identity.public(),
            presence_key: identity.presence_key().clone(),
            name: name.into(),
            paired_at: Timestamp::default(),
        }
    }

    fn device(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            ..Default::default()
        }
    }

    fn config(
        me: &Arc<DeviceIdentity>,
        other: &DeviceIdentity,
        name: &str,
        other_name: &str,
    ) -> NodeConfig {
        let mut config =
            NodeConfig::new(me.clone(), vec![trusted(other, other_name)], device(name));
        config.listen = "127.0.0.1:0".parse().unwrap();
        config.capabilities = vec![capability("page", 1, 1)];
        config
    }

    async fn next<T>(
        events: &mut mpsc::Receiver<NodeEvent>,
        mut pick: impl FnMut(NodeEvent) -> Option<T>,
    ) -> T {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let event = events.recv().await.expect("node stopped");
                if let Some(found) = pick(event) {
                    return found;
                }
            }
        })
        .await
        .expect("timed out waiting for event")
    }

    async fn advert(events: &mut mpsc::Receiver<NodeEvent>) -> Advertisement {
        next(events, |e| match e {
            NodeEvent::Advertise(ad) => Some(ad),
            _ => None,
        })
        .await
    }

    async fn session_up(events: &mut mpsc::Receiver<NodeEvent>) -> (DeviceId, Hello, Negotiated) {
        next(events, |e| match e {
            NodeEvent::SessionUp {
                peer,
                hello,
                capabilities,
            } => Some((peer, hello, capabilities)),
            _ => None,
        })
        .await
    }

    struct Pair {
        a: NodeHandle,
        a_events: mpsc::Receiver<NodeEvent>,
        b: NodeHandle,
        b_events: mpsc::Receiver<NodeEvent>,
        laptop: Arc<DeviceIdentity>,
        phone: Arc<DeviceIdentity>,
        a_ad: Advertisement,
        b_ad: Advertisement,
    }

    fn local(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// Two nodes that have discovered each other via (simulated) mDNS.
    async fn connected_pair() -> Pair {
        let (laptop, phone) = (identity(1), identity(2));
        let (a, mut a_events) = start(config(&laptop, &phone, "Laptop", "Phone"))
            .await
            .unwrap();
        let (b, mut b_events) = start(config(&phone, &laptop, "Phone", "Laptop"))
            .await
            .unwrap();
        let (a_ad, b_ad) = (advert(&mut a_events).await, advert(&mut b_events).await);
        a.observe_lan(
            b_ad.record.name.clone(),
            b_ad.record.txt.clone(),
            local(b.port()),
        )
        .await
        .unwrap();
        b.observe_lan(
            a_ad.record.name.clone(),
            a_ad.record.txt.clone(),
            local(a.port()),
        )
        .await
        .unwrap();
        Pair {
            a,
            a_events,
            b,
            b_events,
            laptop,
            phone,
            a_ad,
            b_ad,
        }
    }

    #[tokio::test]
    async fn nodes_find_each_other_and_negotiate() {
        let mut p = connected_pair().await;
        p.a.observe_ble(p.b_ad.beacon.to_vec(), -50).await.unwrap();

        let (peer, hello, capabilities) = session_up(&mut p.a_events).await;
        assert_eq!(peer, p.phone.device_id());
        assert_eq!(hello.device.unwrap().name, "Phone");
        assert_eq!(capabilities.get("page"), Some(&1));
        assert_eq!(capabilities.get("trust"), Some(&1));
        let (peer, _, _) = session_up(&mut p.b_events).await;
        assert_eq!(peer, p.laptop.device_id());

        let snapshot = p.a.snapshot().await.unwrap();
        let status = &snapshot[0];
        assert!(status.connected && status.reachable && status.nearby);
        assert_eq!(status.band, Some(ProximityBand::Immediate));
        assert!(status.capabilities.contains_key("page"));

        p.a.shutdown().await.unwrap();
        let down = next(&mut p.b_events, |e| match e {
            NodeEvent::SessionDown { peer, .. } => Some(peer),
            _ => None,
        })
        .await;
        assert_eq!(down, p.laptop.device_id());
        let _ = p.a_ad;
    }

    #[tokio::test]
    async fn feature_messages_flow_only_when_negotiated() {
        let mut p = connected_pair().await;
        session_up(&mut p.a_events).await;
        session_up(&mut p.b_events).await;

        p.a.send_feature(p.phone.device_id(), "page", b"ring".to_vec())
            .await
            .unwrap();
        let (capability, version, body) = next(&mut p.b_events, |e| match e {
            NodeEvent::Feature {
                capability,
                version,
                body,
                ..
            } => Some((capability, version, body)),
            _ => None,
        })
        .await;
        assert_eq!(
            (capability.as_str(), version, body.as_slice()),
            ("page", 1, b"ring".as_slice())
        );

        let err =
            p.a.send_feature(p.phone.device_id(), "sidecar", vec![])
                .await
                .unwrap_err();
        assert!(matches!(err, NodeError::NotNegotiated(_)));
        let stranger = identity(9).device_id();
        let err =
            p.a.send_feature(stranger, "page", vec![])
                .await
                .unwrap_err();
        assert!(matches!(err, NodeError::NotConnected));
    }

    #[tokio::test]
    async fn rotated_presence_key_reaches_connected_peer() {
        let mut p = connected_pair().await;
        session_up(&mut p.a_events).await;
        session_up(&mut p.b_events).await;

        let rotated = Arc::new(
            p.laptop
                .with_new_presence_key(&mut DeterministicEntropy::new(42)),
        );
        p.a.set_identity(rotated.clone()).await.unwrap();
        let new_ad = advert(&mut p.a_events).await;
        assert_ne!(new_ad.id, p.a_ad.id);

        let updated = next(&mut p.b_events, |e| match e {
            NodeEvent::PeerUpdated(peer) => Some(peer),
            _ => None,
        })
        .await;
        assert_eq!(updated.device_id(), p.laptop.device_id());
        assert_eq!(&updated.presence_key, rotated.presence_key());

        // The phone can now resolve the laptop's new beacon.
        p.b.observe_ble(new_ad.beacon.to_vec(), -60).await.unwrap();
        let status = p.b.snapshot().await.unwrap().remove(0);
        assert!(status.nearby);
    }

    #[tokio::test]
    async fn unpairing_one_side_unpairs_both() {
        let mut p = connected_pair().await;
        session_up(&mut p.a_events).await;
        session_up(&mut p.b_events).await;

        p.a.unpair(p.phone.device_id()).await.unwrap();
        assert!(p.a.snapshot().await.unwrap().is_empty());
        let gone = next(&mut p.b_events, |e| match e {
            NodeEvent::Unpaired { peer } => Some(peer),
            _ => None,
        })
        .await;
        assert_eq!(gone, p.laptop.device_id());
        assert!(p.b.snapshot().await.unwrap().is_empty());

        let err = p.a.unpair(p.phone.device_id()).await.unwrap_err();
        assert!(matches!(err, NodeError::UnknownPeer));
    }

    #[tokio::test]
    async fn records_seen_before_pairing_are_picked_up() {
        let (laptop, phone) = (identity(1), identity(2));
        let mut cfg = config(&laptop, &phone, "Laptop", "Phone");
        cfg.peers.clear();
        let (a, mut a_events) = start(cfg).await.unwrap();
        advert(&mut a_events).await;
        let id = phone.presence_key().generate_id(&mut DeterministicEntropy::new(5));
        let record = continuum_record(id, 4000, BeaconFlags::LAN);
        a.observe_lan(record.name, record.txt, "127.0.0.1:4000".parse().unwrap())
            .await
            .unwrap();
        assert!(a.snapshot().await.unwrap().is_empty());

        a.set_peers(vec![trusted(&phone, "Phone")]).await.unwrap();
        assert!(a.snapshot().await.unwrap()[0].reachable);
    }

    #[tokio::test]
    async fn untrusted_records_are_ignored() {
        let (laptop, phone, stranger) = (identity(1), identity(2), identity(3));
        let (a, mut a_events) = start(config(&laptop, &phone, "Laptop", "Phone"))
            .await
            .unwrap();
        advert(&mut a_events).await;
        let mut entropy = DeterministicEntropy::new(9);
        let id = stranger.presence_key().generate_id(&mut entropy);
        let record = continuum_record(id, 1, BeaconFlags::LAN);
        a.observe_lan(record.name, record.txt, "127.0.0.1:1".parse().unwrap())
            .await
            .unwrap();
        assert!(!a.snapshot().await.unwrap()[0].reachable);
    }
}
