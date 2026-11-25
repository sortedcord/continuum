//! In-memory simulation of BLE air and a LAN, for testing discovery without
//! hardware.
//!
//! Devices are placed on a 2D plane. BLE beacons go through the real
//! [`Beacon`] codec and get an RSSI from a log-distance path-loss model; LAN
//! records reach every device on the simulated subnet.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use continuum_proto::ResolvableId;
use continuum_proto::beacon::{BEACON_LEN, Beacon, BeaconFlags};

use crate::clock::Timestamp;
use crate::presence::Observation;

/// Log-distance path loss: `rssi = tx_power - 10 * exponent * log10(d)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathLoss {
    /// RSSI at one metre, in dBm.
    pub tx_power: f32,
    /// 2.0 in free space, roughly 2.5 to 4 indoors.
    pub exponent: f32,
    /// Beyond this distance, beacons are not received at all.
    pub range_m: f32,
}

impl Default for PathLoss {
    fn default() -> Self {
        Self {
            tx_power: -59.0,
            exponent: 2.0,
            range_m: 30.0,
        }
    }
}

impl PathLoss {
    pub fn rssi_at(&self, distance_m: f32) -> Option<i16> {
        if distance_m > self.range_m {
            return None;
        }
        let distance = distance_m.max(0.1);
        let rssi = self.tx_power - 10.0 * self.exponent * distance.log10();
        // RSSI is always within i16 range.
        #[allow(clippy::cast_possible_truncation)]
        Some(rssi.round() as i16)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LanRecord {
    id: ResolvableId,
    addr: SocketAddr,
    pairing: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct SimDevice {
    position: (f32, f32),
    beacon: Option<[u8; BEACON_LEN]>,
    lan: Option<LanRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct SimWorld {
    path_loss: PathLoss,
    devices: BTreeMap<String, SimDevice>,
}

impl SimWorld {
    pub fn new(path_loss: PathLoss) -> Self {
        Self {
            path_loss,
            devices: BTreeMap::new(),
        }
    }

    /// Adds a device, or moves it if it already exists.
    pub fn place(&mut self, name: &str, position: (f32, f32)) {
        self.devices.entry(name.to_owned()).or_default().position = position;
    }

    pub fn advertise_ble(&mut self, name: &str, beacon: &Beacon) {
        self.device(name).beacon = Some(beacon.encode());
    }

    pub fn stop_ble(&mut self, name: &str) {
        self.device(name).beacon = None;
    }

    pub fn publish_lan(&mut self, name: &str, id: ResolvableId, addr: SocketAddr, pairing: bool) {
        self.device(name).lan = Some(LanRecord { id, addr, pairing });
    }

    pub fn unpublish_lan(&mut self, name: &str) {
        self.device(name).lan = None;
    }

    /// Everything `observer` would see if it scanned at `at`.
    ///
    /// # Panics
    ///
    /// Panics if `observer` was never placed.
    pub fn scan(&self, observer: &str, at: Timestamp) -> Vec<Observation> {
        let me = &self.devices[observer];
        let mut seen = Vec::new();
        for (name, device) in &self.devices {
            if name == observer {
                continue;
            }
            if let Some(bytes) = &device.beacon
                && let Some(rssi) = self
                    .path_loss
                    .rssi_at(distance(me.position, device.position))
                && let Ok(beacon) = Beacon::decode(bytes)
            {
                let mut obs = Observation::ble(beacon.id, rssi, at);
                obs.pairing = beacon.flags.contains(BeaconFlags::PAIRING);
                seen.push(obs);
            }
            if let Some(record) = &device.lan {
                let mut obs = Observation::lan(record.id, record.addr, at);
                obs.pairing = record.pairing;
                seen.push(obs);
            }
        }
        seen
    }

    fn device(&mut self, name: &str) -> &mut SimDevice {
        self.devices
            .get_mut(name)
            .unwrap_or_else(|| panic!("device {name:?} was never placed"))
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::clock::{Clock, ManualClock};
    use crate::entropy::DeterministicEntropy;
    use crate::identity::DeviceIdentity;
    use crate::presence::{
        PresenceEvent, PresenceRegistry, ProximityBand, RegistryConfig, Transport,
    };
    use crate::rid::{RotatingId, random_id};

    #[test]
    fn path_loss_decreases_with_distance() {
        let model = PathLoss::default();
        assert_eq!(model.rssi_at(1.0), Some(-59));
        assert_eq!(model.rssi_at(10.0), Some(-79));
        assert_eq!(model.rssi_at(31.0), None);
    }

    /// A phone sits next to a laptop, wanders off, and comes back on a
    /// different identifier.
    #[test]
    fn phone_walks_away_and_returns() {
        let mut entropy = DeterministicEntropy::new(1);
        let clock = ManualClock::new(Timestamp::from_millis(0));
        let phone = DeviceIdentity::generate(&mut entropy);
        let peer = phone.device_id();

        let mut world = SimWorld::default();
        world.place("laptop", (0.0, 0.0));
        world.place("phone", (0.3, 0.0));

        let mut registry = PresenceRegistry::new(RegistryConfig::default());
        registry.set_trusted([(peer, phone.presence_key().clone())]);
        let mut rotating = RotatingId::new(phone.presence_key().clone(), Duration::from_secs(60));

        let mut tick = |world: &mut SimWorld, registry: &mut PresenceRegistry| {
            let now = clock.now();
            let (id, _) = rotating.current(now, &mut entropy);
            world.advertise_ble(
                "phone",
                &Beacon {
                    flags: BeaconFlags::empty(),
                    id,
                },
            );
            let mut events = Vec::new();
            for obs in world.scan("laptop", now) {
                events.extend(registry.ingest(&obs));
            }
            events.extend(registry.expire(now));
            clock.advance(Duration::from_secs(1));
            events
        };

        let events = tick(&mut world, &mut registry);
        assert!(events.contains(&PresenceEvent::Appeared {
            peer,
            transport: Transport::Ble
        }));
        assert!(events.contains(&PresenceEvent::ProximityChanged {
            peer,
            band: ProximityBand::Immediate
        }));

        // Walk out of radio range and wait for the TTL.
        world.place("phone", (100.0, 0.0));
        let lost: Vec<_> = (0..31)
            .flat_map(|_| tick(&mut world, &mut registry))
            .collect();
        assert!(lost.contains(&PresenceEvent::Lost {
            peer,
            transport: Transport::Ble
        }));

        // Back at the desk after the identifier has rotated.
        world.place("phone", (0.5, 0.0));
        let events = tick(&mut world, &mut registry);
        assert!(events.contains(&PresenceEvent::Appeared {
            peer,
            transport: Transport::Ble
        }));
    }

    #[test]
    fn strangers_and_pairing_devices_on_the_lan() {
        let mut entropy = DeterministicEntropy::new(2);
        let mut world = SimWorld::default();
        world.place("laptop", (0.0, 0.0));
        world.place("stranger", (50.0, 0.0));
        world.place("new-phone", (50.0, 0.0));
        let addr: SocketAddr = "192.168.1.30:5000".parse().unwrap();
        world.publish_lan("stranger", random_id(&mut entropy), addr, false);
        let candidate = random_id(&mut entropy);
        world.publish_lan("new-phone", candidate, addr, true);

        let mut registry = PresenceRegistry::new(RegistryConfig::default());
        let events: Vec<_> = world
            .scan("laptop", Timestamp::from_millis(0))
            .iter()
            .flat_map(|obs| registry.ingest(obs))
            .collect();
        assert_eq!(
            events,
            vec![PresenceEvent::PairingCandidateAppeared {
                id: candidate,
                transport: Transport::Lan
            }]
        );
    }
}
