//! `continuum run`: the presence daemon. Drives a [`continuum_node`] node
//! with Avahi and BlueZ.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context as _;
use continuum_core::presence::PresenceEvent;
use continuum_core::proto::{BLE_SERVICE_UUID, MDNS_SERVICE_TYPE};
use continuum_linux::avahi::{Avahi, BrowseEvent, Publication};
use continuum_linux::ble::{Ble, Scanner};
use continuum_node::{NodeConfig, NodeEvent, start};

use crate::ctx::{Ctx, short_id};
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
pub async fn run(mut ctx: Ctx, opts: RunOpts) -> anyhow::Result<()> {
    let mut config = NodeConfig::new(ctx.identity.clone(), ctx.peers.clone(), ctx.hello());
    config.listen.set_port(opts.port);
    config.keepalive = opts.keepalive;
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
                NodeEvent::Presence(event) => report(&ctx, event),
                NodeEvent::Peer(status) => tracing::debug!(?status, "peer status"),
                NodeEvent::SessionUp { peer, hello } => {
                    let caps: Vec<String> = hello
                        .manifest
                        .iter()
                        .flat_map(|m| &m.capabilities)
                        .map(|c| format!("{}.v{}", c.name, c.version))
                        .collect();
                    log(format!("session up with \"{}\" [{}]", ctx.peer_name(&peer), caps.join(", ")));
                }
                NodeEvent::SessionDown { peer, reason } => {
                    log(format!("session with \"{}\" ended: {reason}", ctx.peer_name(&peer)));
                }
                NodeEvent::PairingCandidate { .. } => {}
                NodeEvent::Message { peer, body } => {
                    tracing::info!(peer = %ctx.peer_name(&peer), ?body, "message");
                }
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
                if let Ok(peers) = ctx.keyring.load_peers().await
                    && peers != ctx.peers
                {
                    ctx.peers = peers;
                    node.set_peers(ctx.peers.clone()).await?;
                    log(format!("trust store changed: {} paired device(s)", ctx.peers.len()));
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

fn report(ctx: &Ctx, event: PresenceEvent) {
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
            log(format!("\"{}\" is now {band:?}", ctx.peer_name(&peer)));
        }
        _ => {}
    }
}
