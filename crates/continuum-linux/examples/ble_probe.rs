//! Advertises a Continuum beacon for a few seconds while scanning for
//! Continuum beacons and Quick Share triggers.
//! Usage: cargo run -p continuum-linux --example ble_probe
use std::collections::HashSet;
use std::time::Duration;

use continuum_core::entropy::OsEntropy;
use continuum_core::proto::beacon::{Beacon, BeaconFlags};
use continuum_core::rid::random_id;
use continuum_linux::ble::{Ble, uuid16};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ble = Ble::connect().await?;
    println!("adapter: {}", ble.adapter_name());
    let continuum: bluer::Uuid = continuum_core::proto::BLE_SERVICE_UUID.parse()?;
    let beacon = Beacon {
        flags: BeaconFlags::PAIRING,
        id: random_id(&mut OsEntropy),
    };
    let handle = ble.advertise(continuum, beacon.encode().to_vec()).await;
    println!("advertise beacon: {:?}", handle.as_ref().map(|_| "ok"));
    let mut scanner = ble
        .scan(HashSet::from([
            continuum,
            uuid16(continuum_quickshare::ble::SERVICE_UUID16),
        ]))
        .await?;
    let deadline = tokio::time::sleep(Duration::from_secs(8));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            Some(seen) = scanner.next() => println!("{seen:?}"),
            () = &mut deadline => break,
        }
    }
    drop(handle);
    Ok(())
}
