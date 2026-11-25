//! The node event loop.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use continuum_core::clock::{Clock, SystemClock, Timestamp};
use continuum_core::entropy::OsEntropy;
use continuum_core::identity::{DeviceId, DeviceIdentity};
use continuum_core::presence::{
    PresenceEvent, PresenceRegistry, ProximityBand, RegistryConfig, Transport,
};
use continuum_core::proto::ResolvableId;
use continuum_core::proto::beacon::{BEACON_LEN, Beacon, BeaconFlags};
use continuum_core::proto::v1::{Hello, envelope};
use continuum_core::rid::RotatingId;
use continuum_core::trust::TrustedPeer;
use continuum_net::{
    Intent, SessionLink, accept_session, connect_session, read_preamble, spawn_link,
};
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

#[derive(Debug)]
pub struct NodeConfig {
    pub identity: Arc<DeviceIdentity>,
    pub peers: Vec<TrustedPeer>,
    pub hello: Hello,
    /// Where to accept sessions; port 0 picks a free port.
    pub listen: SocketAddr,
    pub keepalive: Duration,
    pub rotation: Duration,
    pub registry: RegistryConfig,
}

impl NodeConfig {
    pub fn new(identity: Arc<DeviceIdentity>, peers: Vec<TrustedPeer>, hello: Hello) -> Self {
        Self {
            identity,
            peers,
            hello,
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
    },
    SessionDown {
        peer: DeviceId,
        reason: String,
    },
    /// A device in pairing mode was seen (or is gone).
    PairingCandidate {
        id: ResolvableId,
        transport: Transport,
        addr: Option<SocketAddr>,
        present: bool,
    },
    /// An application message from a peer.
    Message {
        peer: DeviceId,
        body: envelope::Body,
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
    Send {
        peer: DeviceId,
        body: envelope::Body,
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

    /// Replaces the trust store, e.g. after pairing or unpairing.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn set_peers(&self, peers: Vec<TrustedPeer>) -> Result<(), NodeError> {
        self.command(Command::SetPeers(peers)).await
    }

    /// Sends a message on the session with `peer`, if one is up.
    ///
    /// # Errors
    /// Fails if the node has stopped.
    pub async fn send(&self, peer: DeviceId, body: envelope::Body) -> Result<(), NodeError> {
        self.command(Command::Send { peer, body }).await
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
        reason: String,
    },
    ConnectFailed {
        peer: DeviceId,
    },
    Message {
        peer: DeviceId,
        body: envelope::Body,
    },
}

struct Live {
    sender: mpsc::Sender<envelope::Body>,
    initiated: bool,
}

struct Node {
    identity: Arc<DeviceIdentity>,
    our_id: DeviceId,
    peers: Vec<TrustedPeer>,
    hello: Hello,
    keepalive: Duration,
    port: u16,
    clock: SystemClock,
    rotating: RotatingId,
    registry: PresenceRegistry,
    sessions: HashMap<DeviceId, Live>,
    connecting: HashSet<DeviceId>,
    last_attempt: HashMap<DeviceId, Timestamp>,
    reachable_since: HashMap<DeviceId, Timestamp>,
    events: mpsc::Sender<NodeEvent>,
    internal_tx: mpsc::Sender<Internal>,
    internal_rx: Option<mpsc::Receiver<Internal>>,
}

impl Node {
    fn new(config: NodeConfig, port: u16, events: mpsc::Sender<NodeEvent>) -> Self {
        let mut registry = PresenceRegistry::new(config.registry);
        registry.set_trusted_peers(&config.peers);
        let (internal_tx, internal_rx) = mpsc::channel(64);
        Self {
            our_id: config.identity.device_id(),
            rotating: RotatingId::new(config.identity.presence_key().clone(), config.rotation),
            identity: config.identity,
            peers: config.peers,
            hello: config.hello,
            keepalive: config.keepalive,
            port,
            clock: SystemClock,
            registry,
            sessions: HashMap::new(),
            connecting: HashSet::new(),
            last_attempt: HashMap::new(),
            reachable_since: HashMap::new(),
            events,
            internal_tx,
            internal_rx: Some(internal_rx),
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
        PeerStatus {
            id,
            name: peer.name.clone(),
            nearby: ble.is_some(),
            reachable: lan.is_some(),
            connected: self.sessions.contains_key(&id),
            band: ble.and_then(|b| b.band),
            rssi: ble.and_then(|b| b.rssi),
            addrs: lan.map(|l| rank_addrs(l.addrs())).unwrap_or_default(),
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
                    self.hello.clone(),
                    self.internal_tx.clone(),
                    self.keepalive,
                ));
            }
        }
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
                self.sessions.insert(
                    peer,
                    Live {
                        sender: link.sender(),
                        initiated,
                    },
                );
                let hello = link.hello.clone();
                tokio::spawn(watch(*link, self.internal_tx.clone()));
                self.emit(NodeEvent::SessionUp { peer, hello }).await;
                self.emit_status(&peer).await;
            }
            Internal::Down { peer, reason } => {
                // Ignore the end of a duplicate we already replaced.
                if self
                    .sessions
                    .get(&peer)
                    .is_some_and(|live| live.sender.is_closed())
                {
                    self.sessions.remove(&peer);
                    self.emit(NodeEvent::SessionDown { peer, reason }).await;
                    self.emit_status(&peer).await;
                }
            }
            Internal::ConnectFailed { peer } => {
                self.connecting.remove(&peer);
            }
            Internal::Message { peer, body } => self.emit(NodeEvent::Message { peer, body }).await,
        }
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
                            self.hello.clone(),
                            self.internal_tx.clone(),
                            self.keepalive,
                        ));
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    let now = self.clock.now();
                    match command {
                        Command::Lan { name, txt, addr } => {
                            if let Some(obs) = lan_observation(&name, &txt, addr, now) {
                                let events = self.registry.ingest(&obs);
                                self.handle_presence(events).await;
                            }
                        }
                        Command::LanRemoved { name } => {
                            if let Some(id) = continuum_id(&name) {
                                let events = self.registry.withdraw(Transport::Lan, &id);
                                self.handle_presence(events).await;
                            }
                        }
                        Command::Ble { data, rssi } => {
                            if let Some(obs) = ble_observation(&data, rssi, now) {
                                let events = self.registry.ingest(&obs);
                                self.handle_presence(events).await;
                            }
                        }
                        Command::SetPeers(peers) => {
                            self.peers = peers;
                            let trusted: HashSet<DeviceId> = self.peers.iter().map(TrustedPeer::device_id).collect();
                            // Close sessions with peers that are no longer trusted.
                            self.sessions.retain(|id, _| trusted.contains(id));
                            let events = self.registry.set_trusted_peers(&self.peers);
                            self.handle_presence(events).await;
                        }
                        Command::Send { peer, body } => {
                            if let Some(live) = self.sessions.get(&peer) {
                                let _ = live.sender.send(body).await;
                            }
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
                            return;
                        }
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
    let reason = match closed.await {
        Ok(reason) => format!("{reason:?}"),
        Err(_) => "dropped".into(),
    };
    let _ = internal.send(Internal::Down { peer, reason }).await;
}

#[cfg(test)]
mod tests {
    use continuum_core::clock::Timestamp;
    use continuum_core::entropy::DeterministicEntropy;
    use continuum_core::proto::v1::DeviceInfo;

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

    fn hello(name: &str) -> Hello {
        Hello {
            device: Some(DeviceInfo {
                name: name.into(),
                ..Default::default()
            }),
            manifest: None,
        }
    }

    fn config(
        me: &Arc<DeviceIdentity>,
        other: &DeviceIdentity,
        name: &str,
        other_name: &str,
    ) -> NodeConfig {
        let mut config = NodeConfig::new(me.clone(), vec![trusted(other, other_name)], hello(name));
        config.listen = "127.0.0.1:0".parse().unwrap();
        config
    }

    async fn next_advert(events: &mut mpsc::Receiver<NodeEvent>) -> Advertisement {
        loop {
            if let Some(NodeEvent::Advertise(ad)) = events.recv().await {
                return ad;
            }
        }
    }

    async fn wait_session(events: &mut mpsc::Receiver<NodeEvent>) -> (DeviceId, Hello) {
        loop {
            match events.recv().await {
                Some(NodeEvent::SessionUp { peer, hello }) => return (peer, hello),
                Some(_) => {}
                None => panic!("node stopped"),
            }
        }
    }

    /// Two nodes discover each other via (simulated) mDNS and connect.
    #[tokio::test]
    async fn nodes_find_each_other_and_connect() {
        let (laptop, phone) = (identity(1), identity(2));
        let (a, mut a_events) = start(config(&laptop, &phone, "Laptop", "Phone"))
            .await
            .unwrap();
        let (b, mut b_events) = start(config(&phone, &laptop, "Phone", "Laptop"))
            .await
            .unwrap();
        let (a_ad, b_ad) = (
            next_advert(&mut a_events).await,
            next_advert(&mut b_events).await,
        );

        let local = |port| SocketAddr::from(([127, 0, 0, 1], port));
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
        // BLE sightings feed the same registry.
        a.observe_ble(b_ad.beacon.to_vec(), -50).await.unwrap();

        let timeout = Duration::from_secs(20);
        let (peer, hello) = tokio::time::timeout(timeout, wait_session(&mut a_events))
            .await
            .unwrap();
        assert_eq!(peer, phone.device_id());
        assert_eq!(hello.device.unwrap().name, "Phone");
        let (peer, _) = tokio::time::timeout(timeout, wait_session(&mut b_events))
            .await
            .unwrap();
        assert_eq!(peer, laptop.device_id());

        let snapshot = a.snapshot().await.unwrap();
        assert_eq!(snapshot.len(), 1);
        let status = &snapshot[0];
        assert!(status.connected && status.reachable && status.nearby);
        assert_eq!(status.band, Some(ProximityBand::Immediate));

        a.shutdown().await.unwrap();
        let down = tokio::time::timeout(timeout, async {
            loop {
                if let Some(NodeEvent::SessionDown { peer, .. }) = b_events.recv().await {
                    return peer;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(down, laptop.device_id());
    }

    #[tokio::test]
    async fn untrusted_records_are_ignored() {
        let (laptop, phone, stranger) = (identity(1), identity(2), identity(3));
        let (a, mut a_events) = start(config(&laptop, &phone, "Laptop", "Phone"))
            .await
            .unwrap();
        next_advert(&mut a_events).await;
        let mut entropy = DeterministicEntropy::new(9);
        let id = stranger.presence_key().generate_id(&mut entropy);
        let record = continuum_record(id, 1, BeaconFlags::LAN);
        a.observe_lan(record.name, record.txt, "127.0.0.1:1".parse().unwrap())
            .await
            .unwrap();
        let snapshot = a.snapshot().await.unwrap();
        assert!(!snapshot[0].reachable);
    }
}
