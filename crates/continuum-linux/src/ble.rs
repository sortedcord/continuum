//! BlueZ adapter: advertise service data and scan for it.
//!
//! Generic over the service UUID, so the same code carries Continuum beacons
//! and Quick Share wake-up triggers.

use std::collections::{BTreeMap, HashMap, HashSet};

use std::time::Duration;

use bluer::adv::{Advertisement, AdvertisementHandle, Type};
use bluer::monitor::{Monitor, MonitorHandle, Pattern, RssiSamplingPeriod};
use bluer::{
    Adapter, AdapterEvent, Address, DeviceEvent, DeviceProperty, DiscoveryFilter,
    DiscoveryTransport, Session, Uuid,
};
use futures::{StreamExt, pin_mut};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Bluetooth base UUID form of a 16-bit service UUID.
pub fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128((u128::from(short) << 96) | 0x0000_0000_0000_1000_8000_0080_5f9b_34fb)
}

#[derive(Debug, thiserror::Error)]
pub enum BleError {
    #[error("BlueZ: {0}")]
    Bluez(#[from] bluer::Error),
    #[error("Bluetooth adapter {0} is powered off")]
    PoweredOff(String),
}

/// One received advertisement carrying service data we care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDataSeen {
    pub address: Address,
    pub uuid: Uuid,
    pub data: Vec<u8>,
    pub rssi: Option<i16>,
}

#[derive(Debug, Clone)]
pub struct Ble {
    // Keeps the D-Bus connection alive.
    _session: Session,
    adapter: Adapter,
}

impl Ble {
    /// Uses the default adapter. Never powers it on behind the user's back.
    ///
    /// # Errors
    /// Fails if BlueZ is unavailable or the adapter is off.
    pub async fn connect() -> Result<Self, BleError> {
        let session = Session::new().await?;
        let adapter = session.default_adapter().await?;
        if !adapter.is_powered().await? {
            return Err(BleError::PoweredOff(adapter.name().to_owned()));
        }
        Ok(Self {
            _session: session,
            adapter,
        })
    }

    pub fn adapter_name(&self) -> &str {
        self.adapter.name()
    }

    /// Starts a non-connectable broadcast with one service data entry.
    /// Advertising stops when the handle is dropped.
    ///
    /// # Errors
    /// Fails if BlueZ rejects the advertisement (e.g. too long).
    pub async fn advertise(
        &self,
        uuid: Uuid,
        data: Vec<u8>,
    ) -> Result<AdvertisementHandle, BleError> {
        let advertisement = Advertisement {
            advertisement_type: Type::Broadcast,
            service_data: BTreeMap::from([(uuid, data)]),
            discoverable: Some(false),
            // BlueZ defaults to 1.28 s, too slow for a responsive radar.
            // 200-300 ms matches Android's "balanced" mode.
            min_interval: Some(Duration::from_millis(200)),
            max_interval: Some(Duration::from_millis(300)),
            ..Default::default()
        };
        Ok(self.adapter.advertise(advertisement).await?)
    }

    /// Scans for advertisements with service data under any of `uuids`.
    /// Dropping the returned [`Scanner`] stops scanning.
    ///
    /// # Errors
    /// Fails if discovery cannot start.
    pub async fn scan(&self, uuids: HashSet<Uuid>) -> Result<Scanner, BleError> {
        // No UUID filter: the kernel only matches UUIDs in the advertised
        // service-UUID lists, not service-data UUIDs, which is all our
        // beacons carry. Filtering happens in `watch_device` instead.
        //
        // An RSSI filter, even one that admits everything, makes BlueZ report
        // every RSSI change instead of only changes of 8 dB or more.
        self.adapter
            .set_discovery_filter(DiscoveryFilter {
                transport: DiscoveryTransport::Le,
                duplicate_data: true,
                rssi: Some(-127),
                ..Default::default()
            })
            .await?;
        let monitor = self.monitor(&uuids).await;
        let (tx, rx) = mpsc::channel(256);
        let adapter = self.adapter.clone();
        let task = tokio::spawn(async move {
            if let Err(err) = scan_loop(adapter, uuids, tx).await {
                tracing::warn!(%err, "BLE scan stopped");
            }
        });
        Ok(Scanner {
            rx,
            task,
            _monitor: monitor,
        })
    }

    /// Registers a BlueZ advertisement monitor for our service data. This
    /// turns off the controller's duplicate filtering, so every beacon is
    /// reported instead of roughly one per scan window. Needs bluetoothd
    /// with `Experimental = true`; without it we fall back to plain scanning.
    async fn monitor(&self, uuids: &HashSet<Uuid>) -> Option<MonitorHandle> {
        let patterns: Vec<Pattern> = uuids.iter().map(service_data_pattern).collect();
        let manager = match self.adapter.monitor().await {
            Ok(manager) => manager,
            Err(err) => {
                tracing::info!(%err, "BlueZ advertisement monitors unavailable; RSSI updates will be sparse");
                return None;
            }
        };
        let monitor = Monitor {
            patterns: Some(patterns),
            rssi_sampling_period: Some(RssiSamplingPeriod::All),
            ..Default::default()
        };
        match manager.register(monitor).await {
            Ok(handle) => Some(handle),
            Err(err) => {
                tracing::info!(%err, "could not register advertisement monitor");
                None
            }
        }
    }
}

/// Matches the service-data AD structure for `uuid` (UUIDs are little-endian
/// on air; 16-bit UUIDs use the short form).
fn service_data_pattern(uuid: &Uuid) -> Pattern {
    let value = uuid.as_u128();
    let base = 0x0000_0000_0000_1000_8000_0080_5f9b_34fb_u128;
    if value & !(0xffff_u128 << 96) == base {
        #[allow(clippy::cast_possible_truncation)]
        let short = (value >> 96) as u16;
        Pattern::new(0x16, 0, &short.to_le_bytes())
    } else {
        let mut bytes = uuid.as_bytes().to_vec();
        bytes.reverse();
        Pattern::new(0x21, 0, &bytes)
    }
}

async fn scan_loop(
    adapter: Adapter,
    uuids: HashSet<Uuid>,
    tx: mpsc::Sender<ServiceDataSeen>,
) -> Result<(), BleError> {
    let events = adapter.discover_devices_with_changes().await?;
    pin_mut!(events);
    let mut watchers: HashMap<Address, JoinHandle<()>> = HashMap::new();
    while let Some(event) = events.next().await {
        match event {
            AdapterEvent::DeviceAdded(address) => {
                if watchers.contains_key(&address) {
                    continue;
                }
                let device = adapter.device(address)?;
                let (tx, uuids) = (tx.clone(), uuids.clone());
                watchers.insert(
                    address,
                    tokio::spawn(async move {
                        if let Err(err) = watch_device(device, address, uuids, tx).await {
                            tracing::debug!(%address, %err, "stopped watching device");
                        }
                    }),
                );
            }
            AdapterEvent::DeviceRemoved(address) => {
                if let Some(watcher) = watchers.remove(&address) {
                    watcher.abort();
                }
            }
            AdapterEvent::PropertyChanged(_) => {}
        }
        if tx.is_closed() {
            break;
        }
    }
    for watcher in watchers.into_values() {
        watcher.abort();
    }
    Ok(())
}

async fn watch_device(
    device: bluer::Device,
    address: Address,
    uuids: HashSet<Uuid>,
    tx: mpsc::Sender<ServiceDataSeen>,
) -> Result<(), BleError> {
    let mut rssi = device.rssi().await?;
    let mut service_data = device.service_data().await?.unwrap_or_default();
    let emit = |rssi: Option<i16>, service_data: &HashMap<Uuid, Vec<u8>>| {
        service_data
            .iter()
            .filter(|(uuid, _)| uuids.contains(uuid))
            .map(|(uuid, data)| ServiceDataSeen {
                address,
                uuid: *uuid,
                data: data.clone(),
                rssi,
            })
            .collect::<Vec<_>>()
    };
    for seen in emit(rssi, &service_data) {
        tx.send(seen).await.ok();
    }
    let events = device.events().await?;
    pin_mut!(events);
    while let Some(DeviceEvent::PropertyChanged(property)) = events.next().await {
        match property {
            DeviceProperty::Rssi(value) => rssi = Some(value),
            DeviceProperty::ServiceData(data) => service_data = data,
            _ => continue,
        }
        for seen in emit(rssi, &service_data) {
            if tx.send(seen).await.is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct Scanner {
    rx: mpsc::Receiver<ServiceDataSeen>,
    task: JoinHandle<()>,
    _monitor: Option<MonitorHandle>,
}

impl Scanner {
    pub async fn next(&mut self) -> Option<ServiceDataSeen> {
        self.rx.recv().await
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_use_on_air_byte_order() {
        let short = service_data_pattern(&uuid16(0xFE2C));
        assert_eq!(
            (short.data_type, short.content.as_slice()),
            (0x16, [0x2C, 0xFE].as_slice())
        );
        let long: Uuid = "9b4354e1-15ec-4324-9f00-e9c5549237f7".parse().unwrap();
        let long = service_data_pattern(&long);
        assert_eq!(long.data_type, 0x21);
        assert_eq!(long.content[0], 0xf7);
        assert_eq!(long.content[15], 0x9b);
    }

    #[test]
    fn expands_16_bit_uuids() {
        assert_eq!(
            uuid16(0xFE2C).to_string(),
            "0000fe2c-0000-1000-8000-00805f9b34fb"
        );
    }
}
