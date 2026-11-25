//! `continuum run`: the presence daemon. Drives a [`continuum_node`] node
//! with Avahi and BlueZ, and serves the D-Bus API.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use continuum_core::presence::PresenceEvent;
use continuum_core::proto::{BLE_SERVICE_UUID, MDNS_SERVICE_TYPE};
use continuum_linux::avahi::{Avahi, BrowseEvent, Publication};
use continuum_linux::ble::{Ble, Scanner};
use continuum_node::features::page::{self, PageAction};
use continuum_node::{NodeConfig, NodeEvent, start};
use zbus::object_server::SignalEmitter;

use crate::ctx::{Ctx, short_id};
use crate::dbus::{self, DaemonService, PeerInfo, Shared};
use crate::ring::Ringer;
use crate::util::now;

#[derive(Debug, Clone)]
pub struct RunOpts {
    pub port: u16,
    pub ble: bool,
    pub keepalive: Duration,
}

const RELOAD_PEERS_EVERY: Duration = Duration::from_secs(30);

fn log(message: impl std::fmt::Display) {
    let secs = now().as_millis() / 1000;
    println!(
        "[{:02}:{:02}:{:02}Z] {message}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    );
}

#[allow(clippy::too_many_lines)]
pub async fn run(ctx: Ctx, opts: RunOpts) -> anyhow::Result<()> {
    let mut config = NodeConfig::new(ctx.identity.clone(), ctx.peers.clone(), ctx.device.clone());
    config.listen.set_port(opts.port);
    config.keepalive = opts.keepalive;
    config.capabilities = vec![page::capability_v1()];
    let (node, mut events) = start(config)
        .await
        .with_context(|| format!("binding TCP port {}", opts.port))?;
    log(format!(
        "{} ({}) listening on port {}, {} paired device(s)",
        ctx.device.name,
        short_id(&ctx.identity.device_id()),
        node.port(),
        ctx.peers.len()
    ));

    let Ctx {
        profile,
        keyring,
        identity,
        peers,
        device,
    } = ctx;
    let shared = Arc::new(Shared {
        node: node.clone(),
        keyring,
        identity: Mutex::new(identity),
        peers: Mutex::new(peers),
        name: device.name,
    });

    let bus = zbus::connection::Builder::session()
        .name(dbus::bus_name(&profile))
        .serve_at(
            dbus::OBJECT_PATH,
            DaemonService {
                shared: shared.clone(),
            },
        )
        .build()
        .await
        .context("registering on the session bus (is another daemon running for this profile?)")?;
    let emitter = SignalEmitter::new(&bus, dbus::OBJECT_PATH)?;

    let avahi = Avahi::connect().await.context("connecting to Avahi")?;
    let mut browser = avahi.browse(MDNS_SERVICE_TYPE).await?;
    let mut publication: Option<Publication> = None;

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
    let mut scanner: Option<Scanner> = match &ble {
        Some(ble) => ble.scan(HashSet::from([ble_uuid])).await.ok(),
        None => None,
    };
    let mut advertisement = None;
    let mut ringer = Ringer::default();

    let mut reload = tokio::time::interval(RELOAD_PEERS_EVERY);
    reload.tick().await;

    loop {
        tokio::select! {
            Some(event) = events.recv() => match event {
                NodeEvent::Advertise(ad) => {
                    publication = match publication.take() {
                        Some(p) => { p.update(&ad.record).await?; Some(p) }
                        None => Some(avahi.publish(&ad.record).await?),
                    };
                    if let Some(ble) = &ble {
                        // Stop the old beacon before starting the new one.
                        drop(advertisement.take());
                        advertisement = ble
                            .advertise(ble_uuid, ad.beacon.to_vec())
                            .await
                            .map_err(|err| log(format!("BLE advertising failed: {err}")))
                            .ok();
                    }
                    tracing::info!(id = %ad.id, "advertising");
                }
                NodeEvent::Presence(event) => report(&shared, event),
                NodeEvent::Peer(status) => {
                    let _ = DaemonService::peer_changed(&emitter, PeerInfo::from(&status)).await;
                }
                NodeEvent::SessionUp { peer, capabilities, .. } => {
                    let caps: Vec<String> = capabilities.iter().map(|(n, v)| format!("{n}.v{v}")).collect();
                    log(format!("session up with \"{}\" [{}]", shared.peer_name(&peer), caps.join(", ")));
                }
                NodeEvent::SessionDown { peer, reason } => {
                    ringer.stop(&peer);
                    log(format!("session with \"{}\" ended: {reason}", shared.peer_name(&peer)));
                }
                NodeEvent::PeerUpdated(updated) => {
                    let mut peers = shared.peers();
                    if let Some(stored) = peers.iter_mut().find(|p| p.device_id() == updated.device_id()) {
                        *stored = updated.clone();
                    }
                    shared.keyring.save_peers(&peers).await?;
                    shared.set_peers(peers);
                    log(format!("\"{}\" rotated its presence key", updated.name));
                }
                NodeEvent::Unpaired { peer } => {
                    let name = shared.peer_name(&peer);
                    let mut peers = shared.peers();
                    peers.retain(|p| p.device_id() != peer);
                    shared.keyring.save_peers(&peers).await?;
                    shared.set_peers(peers);
                    log(format!("\"{name}\" unpaired this device"));
                }
                NodeEvent::Feature { peer, capability, body, .. } if capability == page::CAPABILITY => {
                    let name = shared.peer_name(&peer);
                    match page::decode(&body) {
                        Some(PageAction::Start { duration_s }) => {
                            log(format!("\"{name}\" is paging this computer"));
                            ringer.start(peer, name, Duration::from_secs(duration_s.into()), node.clone());
                        }
                        Some(PageAction::Stop) => ringer.stop(&peer),
                        Some(PageAction::Found) => {
                            log(format!("\"{name}\" was found"));
                            let _ = DaemonService::page_found(&emitter, &peer.to_string(), &name).await;
                        }
                        None => {}
                    }
                }
                NodeEvent::Feature { .. } | NodeEvent::PairingCandidate { .. } => {}
            },
            Some(event) = browser.next() => match event {
                BrowseEvent::Resolved(service) => {
                    node.observe_lan(service.name, service.txt, service.addr).await?;
                }
                BrowseEvent::Removed { name } => node.withdraw_lan(name).await?,
            },
            Some(seen) = async { scanner.as_mut()?.next().await } => {
                if let Some(rssi) = seen.rssi {
                    node.observe_ble(seen.data, rssi).await?;
                }
            }
            _ = reload.tick() => {
                // Pick up pairings made with `continuum pair` while running.
                if let Ok(peers) = shared.keyring.load_peers().await
                    && peers != shared.peers()
                {
                    node.set_peers(peers.clone()).await?;
                    log(format!("trust store changed: {} paired device(s)", peers.len()));
                    shared.set_peers(peers);
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    log("shutting down");
    node.shutdown().await.ok();
    drop(advertisement);
    if let Some(p) = publication {
        p.withdraw().await.ok();
    }
    Ok(())
}

fn report(shared: &Shared, event: PresenceEvent) {
    match event {
        PresenceEvent::Appeared { peer, transport } => {
            log(format!(
                "\"{}\" appeared via {transport:?}",
                shared.peer_name(&peer)
            ));
        }
        PresenceEvent::Lost { peer, transport } => {
            log(format!(
                "\"{}\" lost via {transport:?}",
                shared.peer_name(&peer)
            ));
        }
        PresenceEvent::ProximityChanged { peer, band } => {
            log(format!("\"{}\" is now {band:?}", shared.peer_name(&peer)));
        }
        _ => {}
    }
}
