//! `continuum quickshare`: discovery-level Quick Share compatibility.
//! Transfers are not implemented yet (Checkpoint 3).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use continuum_core::entropy::OsEntropy;
use continuum_linux::avahi::{Avahi, BrowseEvent, Publication};
use continuum_linux::ble::{Ble, uuid16};
use continuum_linux::discovery::{quickshare_endpoint, quickshare_spec};
use continuum_quickshare::ble::{SERVICE_UUID16, is_trigger, trigger_service_data};
use continuum_quickshare::mdns::SERVICE_TYPE;
use continuum_quickshare::visibility::{ReceiverVisibility, Visibility};
use continuum_quickshare::{DeviceType, EndpointId, EndpointInfo};
use tokio::net::TcpListener;

use crate::ctx::Ctx;
use crate::util::now;

/// Wakes nearby Android receivers with the BLE trigger and lists every
/// Quick Share receiver that shows up on the LAN.
pub async fn scan(duration: Duration) -> anyhow::Result<()> {
    let _trigger = match Ble::connect().await {
        Ok(ble) => {
            let data = trigger_service_data(&mut OsEntropy).to_vec();
            match ble.advertise(uuid16(SERVICE_UUID16), data).await {
                Ok(handle) => Some((ble, handle)),
                Err(err) => {
                    eprintln!(
                        "Could not send the Quick Share BLE trigger ({err}); only always-visible receivers will show."
                    );
                    None
                }
            }
        }
        Err(err) => {
            eprintln!("Bluetooth unavailable ({err}); only always-visible receivers will show.");
            None
        }
    };
    let avahi = Avahi::connect().await?;
    let mut browser = avahi.browse(SERVICE_TYPE).await?;
    println!(
        "Looking for Quick Share receivers for {} s ...",
        duration.as_secs()
    );
    let mut seen = HashSet::new();
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            Some(event) = browser.next() => match event {
                BrowseEvent::Resolved(service) => {
                    if let Some(endpoint) = quickshare_endpoint(&service)
                        && seen.insert((endpoint.id, endpoint.addr))
                    {
                        let name = endpoint.info.name.as_deref().unwrap_or("<hidden>");
                        println!(
                            "  {name:<24} {:?}  endpoint {}  at {}",
                            endpoint.info.device_type, endpoint.id, endpoint.addr
                        );
                    }
                }
                BrowseEvent::Removed { name } => println!("  (gone: {name})"),
            },
            () = &mut deadline => break,
        }
    }
    if seen.is_empty() {
        println!("No receivers found. On Android, open the share sheet and pick Quick Share.");
    }
    Ok(())
}

/// Watches for Android devices that are trying to share, and optionally
/// shows this machine as a receiver while they are.
pub async fn watch(ctx: &Ctx, duration: Duration, visible: bool) -> anyhow::Result<()> {
    let ble = Ble::connect().await?;
    let trigger_uuid = uuid16(SERVICE_UUID16);
    let mut scanner = ble.scan(HashSet::from([trigger_uuid])).await?;
    let avahi = Avahi::connect().await?;
    let listener = TcpListener::bind("[::]:0").await?;
    let port = listener.local_addr()?.port();

    let endpoint_id = EndpointId::generate(&mut OsEntropy);
    let info = EndpointInfo::visible(&ctx.device.name, DeviceType::Laptop, &mut OsEntropy);
    let mut visibility = ReceiverVisibility::new(if visible {
        Visibility::default()
    } else {
        Visibility::Off
    });
    let mut publication: Option<Publication> = None;
    let mut last_seen: HashMap<bluer::Address, i16> = HashMap::new();

    println!(
        "Watching for nearby Quick Share senders for {} s ...",
        duration.as_secs()
    );
    if visible {
        println!(
            "This machine will appear as \"{}\" while one is nearby.",
            ctx.device.name
        );
        println!("Transfers are not supported yet; Android will fail to connect.");
    }
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            Some(seen) = scanner.next() => {
                if seen.uuid == trigger_uuid && is_trigger(&seen.data) {
                    visibility.on_trigger(now());
                    let rssi = seen.rssi.unwrap_or_default();
                    if last_seen.insert(seen.address, rssi).is_none() {
                        println!("  Quick Share sender nearby: {} ({rssi} dBm)", seen.address);
                    }
                }
            }
            accepted = listener.accept() => {
                if let Ok((_, from)) = accepted {
                    println!("  Quick Share connection from {from} (transfers arrive in Checkpoint 3)");
                }
            }
            _ = tick.tick() => {
                let should = visibility.should_advertise(now());
                match (should, publication.is_some()) {
                    (true, false) => {
                        publication = Some(avahi.publish(&quickshare_spec(&endpoint_id, &info, port)?).await?);
                        println!("  visible as \"{}\"", ctx.device.name);
                    }
                    (false, true) => {
                        if let Some(p) = publication.take() {
                            p.withdraw().await.ok();
                        }
                        println!("  hidden again");
                    }
                    _ => {}
                }
            }
            () = &mut deadline => break,
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    if let Some(p) = publication {
        p.withdraw().await.ok();
    }
    Ok(())
}
