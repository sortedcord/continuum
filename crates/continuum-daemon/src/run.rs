//! `continuum run`: the presence daemon.
//!
//! Advertises a rotating identifier over mDNS (and BLE when available),
//! tracks paired peers in the presence registry, and keeps one encrypted
//! session per reachable peer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use continuum_core::clock::Timestamp;
use continuum_core::entropy::OsEntropy;
use continuum_core::identity::{DeviceId, DeviceIdentity};
use continuum_core::presence::{PresenceEvent, PresenceRegistry, RegistryConfig, Transport};
use continuum_core::proto::beacon::{Beacon, BeaconFlags};
use continuum_core::proto::v1::{Hello, envelope};
use continuum_core::proto::{BLE_SERVICE_UUID, MDNS_SERVICE_TYPE};
use continuum_core::rid::RotatingId;
use continuum_core::trust::TrustedPeer;
use continuum_linux::avahi::{Avahi, BrowseEvent};
use continuum_linux::ble::{Ble, Scanner};
use continuum_linux::discovery::{
    beacon_observation, continuum_id, continuum_observation, continuum_spec,
};
use continuum_net::{
    Intent, SessionLink, accept_session, connect_session, read_preamble, spawn_link,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::ctx::Ctx;
use crate::util::{connect_any, now, rank_addrs};

#[derive(Debug, Clone)]
pub struct RunOpts {
    pub port: u16,
    pub ble: bool,
    pub keepalive: Duration,
}

/// The higher-id side waits this long for the lower-id side to connect
/// before trying itself, which avoids duplicate sessions.
const CONNECT_GRACE: Duration = Duration::from_secs(10);
const RETRY_AFTER: Duration = Duration::from_secs(15);
const RELOAD_PEERS_EVERY: Duration = Duration::from_secs(30);

enum Event {
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
        error: String,
    },
}

struct Live {
    sender: mpsc::Sender<envelope::Body>,
    initiated: bool,
}

fn log(message: impl std::fmt::Display) {
    let secs = now().as_millis() / 1000;
    println!(
        "[{:02}:{:02}:{:02}] {message}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    );
}

#[allow(clippy::too_many_lines)]
pub async fn run(mut ctx: Ctx, opts: RunOpts) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("::", opts.port))
        .await
        .with_context(|| format!("binding TCP port {}", opts.port))?;
    let port = listener.local_addr()?.port();
    let our_id = ctx.identity.device_id();
    log(format!(
        "{} ({}) listening on port {port}, {} paired device(s)",
        ctx.device.name,
        crate::ctx::short_id(&our_id),
        ctx.peers.len()
    ));

    let mut rotating = RotatingId::new(
        ctx.identity.presence_key().clone(),
        RotatingId::DEFAULT_INTERVAL,
    );
    let (mut current_id, _) = rotating.current(now(), &mut OsEntropy);
    let flags = BeaconFlags::LAN;

    let avahi = Avahi::connect().await.context("connecting to Avahi")?;
    let publication = avahi
        .publish(&continuum_spec(current_id, port, flags))
        .await?;
    let mut browser = avahi.browse(MDNS_SERVICE_TYPE).await?;

    let ble_uuid: bluer::Uuid = BLE_SERVICE_UUID.parse()?;
    let ble = if opts.ble {
        match Ble::connect().await {
            Ok(ble) => Some(ble),
            Err(err) => {
                log(format!("Bluetooth unavailable ({err}); LAN only"));
                None
            }
        }
    } else {
        None
    };
    let advertise = async |ble: &Option<Ble>, id| match ble {
        Some(ble) => ble
            .advertise(ble_uuid, Beacon { flags, id }.encode().to_vec())
            .await
            .map_err(|err| log(format!("BLE advertising failed: {err}")))
            .ok(),
        None => None,
    };
    let mut advertisement = advertise(&ble, current_id).await;
    let mut scanner: Option<Scanner> = match &ble {
        Some(ble) => ble.scan(HashSet::from([ble_uuid])).await.ok(),
        None => None,
    };

    let mut registry = PresenceRegistry::new(RegistryConfig::default());
    registry.set_trusted_peers(&ctx.peers);

    let (events_tx, mut events) = mpsc::channel::<Event>(32);
    let mut sessions: HashMap<DeviceId, Live> = HashMap::new();
    let mut connecting: HashSet<DeviceId> = HashSet::new();
    let mut last_attempt: HashMap<DeviceId, Timestamp> = HashMap::new();
    let mut reachable_since: HashMap<DeviceId, Timestamp> = HashMap::new();
    let mut last_reload = now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        let mut presence_events = Vec::new();
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                tokio::spawn(incoming(
                    stream,
                    ctx.identity.clone(),
                    ctx.peers.clone(),
                    ctx.hello(),
                    events_tx.clone(),
                    opts.keepalive,
                ));
            }
            Some(event) = browser.next() => match event {
                BrowseEvent::Resolved(service) => {
                    if let Some(obs) = continuum_observation(&service, now()) {
                        presence_events = registry.ingest(&obs);
                    }
                }
                BrowseEvent::Removed { name } => {
                    if let Some(id) = continuum_id(&name) {
                        presence_events = registry.withdraw(Transport::Lan, &id);
                    }
                }
            },
            Some(seen) = async { scanner.as_mut()?.next().await } => {
                if let Some(obs) = beacon_observation(&seen, now()) {
                    presence_events = registry.ingest(&obs);
                }
            }
            Some(event) = events.recv() => match event {
                Event::Up { link, initiated } => {
                    let peer = link.peer.device_id();
                    connecting.remove(&peer);
                    // Keep the session started by the lower device id.
                    let preferred = initiated == (our_id < peer);
                    if let Some(existing) = sessions.get(&peer)
                        && (existing.initiated == initiated || !preferred)
                    {
                        continue;
                    }
                    let name = link.hello.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                    let caps: Vec<String> = link
                        .hello
                        .manifest
                        .iter()
                        .flat_map(|m| &m.capabilities)
                        .map(|c| format!("{}.v{}", c.name, c.version))
                        .collect();
                    log(format!("session up with \"{name}\" [{}]", caps.join(", ")));
                    sessions.insert(peer, Live { sender: link.sender(), initiated });
                    tokio::spawn(watch(*link, events_tx.clone()));
                }
                Event::Down { peer, reason } => {
                    // Ignore the end of a duplicate we already dropped.
                    if sessions.get(&peer).is_some_and(|live| live.sender.is_closed()) {
                        sessions.remove(&peer);
                        log(format!("session with \"{}\" ended: {reason}", ctx.peer_name(&peer)));
                    }
                }
                Event::ConnectFailed { peer, error } => {
                    connecting.remove(&peer);
                    tracing::info!(peer = %ctx.peer_name(&peer), %error, "connect failed");
                }
            },
            _ = tick.tick() => {
                let at = now();
                presence_events = registry.expire(at);

                let (id, rotated) = rotating.current(at, &mut OsEntropy);
                if rotated {
                    current_id = id;
                    publication.update(&continuum_spec(current_id, port, flags)).await?;
                    // Stop the old beacon before starting the new one.
                    drop(advertisement.take());
                    advertisement = advertise(&ble, current_id).await;
                    tracing::info!(%current_id, "rotated identifier");
                }

                if at.saturating_since(last_reload) >= RELOAD_PEERS_EVERY {
                    last_reload = at;
                    if let Ok(peers) = ctx.keyring.load_peers().await
                        && peers != ctx.peers
                    {
                        ctx.peers = peers;
                        presence_events.extend(registry.set_trusted_peers(&ctx.peers));
                        log(format!("trust store changed: {} paired device(s)", ctx.peers.len()));
                    }
                }

                for (peer, presence) in registry.peers() {
                    let Some(lan) = &presence.lan else { continue };
                    if sessions.contains_key(peer) || connecting.contains(peer) {
                        continue;
                    }
                    let since = *reachable_since.entry(*peer).or_insert(at);
                    let our_turn = our_id < *peer || at.saturating_since(since) >= CONNECT_GRACE;
                    let backed_off = last_attempt
                        .get(peer)
                        .is_some_and(|t| at.saturating_since(*t) < RETRY_AFTER);
                    let Some(trusted) = ctx.peers.iter().find(|p| &p.device_id() == peer) else { continue };
                    if our_turn && !backed_off {
                        connecting.insert(*peer);
                        last_attempt.insert(*peer, at);
                        tokio::spawn(outgoing(
                            rank_addrs(lan.addrs()),
                            ctx.identity.clone(),
                            trusted.clone(),
                            ctx.hello(),
                            events_tx.clone(),
                            opts.keepalive,
                        ));
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }

        for event in presence_events {
            report(&ctx, &registry, event);
            if let PresenceEvent::Lost {
                peer,
                transport: Transport::Lan,
            } = event
            {
                reachable_since.remove(&peer);
            }
        }
    }

    log("shutting down");
    drop(sessions);
    drop(advertisement);
    publication.withdraw().await.ok();
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok(())
}

fn report(ctx: &Ctx, registry: &PresenceRegistry, event: PresenceEvent) {
    match event {
        PresenceEvent::Appeared { peer, transport } => {
            log(format!(
                "\"{}\" appeared via {transport:?}",
                ctx.peer_name(&peer)
            ));
        }
        PresenceEvent::Lost { peer, transport } => {
            log(format!(
                "\"{}\" lost via {transport:?}",
                ctx.peer_name(&peer)
            ));
        }
        PresenceEvent::ProximityChanged { peer, band } => {
            let rssi = registry
                .presence(&peer)
                .and_then(|p| p.ble.as_ref())
                .and_then(|ble| ble.rssi)
                .unwrap_or_default();
            log(format!(
                "\"{}\" is now {band:?} ({rssi:.0} dBm)",
                ctx.peer_name(&peer)
            ));
        }
        PresenceEvent::AddressesChanged { peer } => {
            let addrs = registry
                .presence(&peer)
                .and_then(|p| p.lan.as_ref())
                .map(|lan| rank_addrs(lan.addrs()))
                .unwrap_or_default();
            tracing::debug!(peer = %ctx.peer_name(&peer), ?addrs, "addresses changed");
        }
        PresenceEvent::PairingCandidateAppeared { .. }
        | PresenceEvent::PairingCandidateLost { .. } => {}
    }
}

async fn incoming(
    mut stream: TcpStream,
    identity: Arc<DeviceIdentity>,
    peers: Vec<TrustedPeer>,
    hello: Hello,
    events: mpsc::Sender<Event>,
    keepalive: Duration,
) {
    stream.set_nodelay(true).ok();
    let from = stream.peer_addr().ok();
    let Ok(Ok(intent)) =
        tokio::time::timeout(Duration::from_secs(10), read_preamble(&mut stream)).await
    else {
        return;
    };
    if intent != Intent::Session {
        tracing::info!(
            ?from,
            ?intent,
            "ignoring non-session connection (use `continuum pair`)"
        );
        return;
    }
    let accepted = tokio::time::timeout(
        Duration::from_secs(10),
        accept_session(stream, &identity, &peers, hello),
    )
    .await;
    match accepted {
        Ok(Ok(session)) => {
            let link = spawn_link(session, keepalive);
            let _ = events
                .send(Event::Up {
                    link: Box::new(link),
                    initiated: false,
                })
                .await;
        }
        Ok(Err(err)) => tracing::info!(?from, %err, "rejected session"),
        Err(_) => tracing::info!(?from, "session handshake timed out"),
    }
}

async fn outgoing(
    addrs: Vec<std::net::SocketAddr>,
    identity: Arc<DeviceIdentity>,
    peer: TrustedPeer,
    hello: Hello,
    events: mpsc::Sender<Event>,
    keepalive: Duration,
) {
    let id = peer.device_id();
    let result = async {
        let stream = connect_any(&addrs).await?;
        let session = tokio::time::timeout(
            Duration::from_secs(10),
            connect_session(stream, &identity, &peer, hello),
        )
        .await??;
        anyhow::Ok(spawn_link(session, keepalive))
    }
    .await;
    let event = match result {
        Ok(link) => Event::Up {
            link: Box::new(link),
            initiated: true,
        },
        Err(err) => Event::ConnectFailed {
            peer: id,
            error: err.to_string(),
        },
    };
    let _ = events.send(event).await;
}

async fn watch(link: SessionLink, events: mpsc::Sender<Event>) {
    let (peer, _, mut incoming, closed) = link.into_receivers();
    let peer = peer.device_id();
    tokio::spawn(async move {
        while let Some(body) = incoming.recv().await {
            tracing::info!(?body, "message from peer");
        }
    });
    let reason = match closed.await {
        Ok(reason) => format!("{reason:?}"),
        Err(_) => "dropped".into(),
    };
    let _ = events.send(Event::Down { peer, reason }).await;
}
