//! `continuum pair`: show an invite, join one, or pair by comparing codes.

use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context as _;
use continuum_core::clock::SystemClock;
use continuum_core::entropy::OsEntropy;
use continuum_core::pairing::Invite;
use continuum_core::proto::beacon::{Beacon, BeaconFlags};
use continuum_core::proto::{BLE_SERVICE_UUID, MDNS_SERVICE_TYPE};
use continuum_core::rid::random_id;
use continuum_core::session::Sas;
use continuum_core::trust::TrustedPeer;
use continuum_linux::avahi::{Avahi, BrowseEvent};
use continuum_linux::ble::Ble;
use continuum_linux::discovery::{continuum_observation, continuum_spec};
use continuum_net::{Intent, PairingMethod, pair_initiator, pair_responder, read_preamble};
use tokio::net::TcpListener;

use crate::ctx::{Ctx, short_id};
use crate::util::{confirm, connect_any, local_addrs, now, print_qr};

async fn confirm_sas(sas: Sas, peer: TrustedPeer) -> bool {
    println!();
    println!(
        "Pairing with \"{}\" ({})",
        peer.name,
        short_id(&peer.device_id())
    );
    println!("    Code: {sas}");
    confirm("Does the same code show on the other device?").await
}

async fn saved(ctx: &mut Ctx, peer: TrustedPeer) -> anyhow::Result<()> {
    println!(
        "Paired with \"{}\" ({}).",
        peer.name,
        short_id(&peer.device_id())
    );
    ctx.save_peer(peer).await
}

/// Waits to be paired: shows a QR code, and also accepts code comparison.
pub async fn invite(ctx: &mut Ctx, ttl: Duration, allow_sas: bool) -> anyhow::Result<()> {
    let listener = TcpListener::bind("[::]:0").await?;
    let port = listener.local_addr()?.port();
    let invite = Invite::new(
        local_addrs(port),
        &ctx.device.name,
        now(),
        ttl,
        &mut OsEntropy,
    );
    anyhow::ensure!(!invite.addrs.is_empty(), "no usable network interface");

    let text = invite.encode();
    print_qr(&text);
    println!("Scan with the Continuum Android app, or on another Linux machine run:");
    println!("    continuum pair join '{text}'");
    if allow_sas {
        println!("Devices without a camera can pair by code: continuum pair scan");
    }
    println!("Waiting {} s ...", ttl.as_secs());

    // Advertise that we are in pairing mode, with an unresolvable id.
    let id = random_id(&mut OsEntropy);
    let avahi = Avahi::connect().await?;
    let _mdns = avahi
        .publish(&continuum_spec(
            id,
            port,
            BeaconFlags::PAIRING.with(BeaconFlags::LAN),
        ))
        .await?;
    let _ble = match Ble::connect().await {
        Ok(ble) => {
            let beacon = Beacon {
                flags: BeaconFlags::PAIRING.with(BeaconFlags::LAN),
                id,
            };
            ble.advertise(BLE_SERVICE_UUID.parse()?, beacon.encode().to_vec())
                .await
                .map_err(|err| tracing::warn!(%err, "BLE pairing beacon unavailable"))
                .ok()
                .map(|handle| (ble, handle))
        }
        Err(err) => {
            tracing::warn!(%err, "Bluetooth unavailable; pairing over the LAN only");
            None
        }
    };

    let deadline = tokio::time::sleep(ttl);
    tokio::pin!(deadline);
    loop {
        let (mut stream, from) = tokio::select! {
            accepted = listener.accept() => accepted?,
            () = &mut deadline => anyhow::bail!("invite expired"),
        };
        let Ok(Ok(intent)) =
            tokio::time::timeout(Duration::from_secs(10), read_preamble(&mut stream)).await
        else {
            continue;
        };
        let method = match intent {
            Intent::PairWithSecret if !invite.is_expired(now()) => {
                PairingMethod::Secret(&invite.secret)
            }
            Intent::PairWithSas if allow_sas => PairingMethod::Sas,
            _ => {
                tracing::info!(%from, ?intent, "ignoring connection");
                continue;
            }
        };
        let result = pair_responder(
            stream,
            &ctx.identity,
            ctx.device.clone(),
            method,
            confirm_sas,
            &SystemClock,
        )
        .await;
        match result {
            Ok(peer) => return saved(ctx, peer).await,
            Err(err) => eprintln!("Pairing attempt from {from} failed: {err}"),
        }
    }
}

/// Joins an invite shown by another device.
pub async fn join(ctx: &mut Ctx, text: &str) -> anyhow::Result<()> {
    let invite = Invite::decode(text).context("reading invite")?;
    anyhow::ensure!(!invite.is_expired(now()), "this invite has expired");
    println!("Pairing with \"{}\" ...", invite.name);
    let stream = connect_any(&invite.addrs).await?;
    let peer = pair_initiator(
        stream,
        &ctx.identity,
        ctx.device.clone(),
        PairingMethod::Secret(&invite.secret),
        confirm_sas,
        &SystemClock,
    )
    .await?;
    saved(ctx, peer).await
}

/// Pairs by code comparison with a device at `addr`.
pub async fn join_sas(ctx: &mut Ctx, addrs: &[SocketAddr]) -> anyhow::Result<()> {
    let stream = connect_any(addrs).await?;
    let peer = pair_initiator(
        stream,
        &ctx.identity,
        ctx.device.clone(),
        PairingMethod::Sas,
        confirm_sas,
        &SystemClock,
    )
    .await?;
    saved(ctx, peer).await
}

/// Lists devices on the LAN that are waiting to be paired.
pub async fn scan(duration: Duration) -> anyhow::Result<Vec<Vec<SocketAddr>>> {
    let avahi = Avahi::connect().await?;
    let mut browser = avahi.browse(MDNS_SERVICE_TYPE).await?;
    let mut found: BTreeMap<String, HashSet<SocketAddr>> = BTreeMap::new();
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            Some(event) = browser.next() => {
                if let BrowseEvent::Resolved(service) = event
                    && let Some(obs) = continuum_observation(&service, now())
                    && obs.pairing
                    && let Some(addr) = obs.addr
                {
                    found.entry(service.name).or_default().insert(addr);
                }
            }
            () = &mut deadline => break,
        }
    }
    Ok(found.into_values().map(crate::util::rank_addrs).collect())
}
