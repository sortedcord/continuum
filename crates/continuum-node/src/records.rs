//! Continuum's discovery records, independent of the mDNS or BLE stack.

use std::net::SocketAddr;

use continuum_core::clock::Timestamp;
use continuum_core::presence::Observation;
use continuum_core::proto::beacon::{Beacon, BeaconFlags};
use continuum_core::proto::{MDNS_SERVICE_TYPE, ResolvableId};

const TXT_VERSION: &str = "v";
const TXT_FLAGS: &str = "f";
const VERSION: &str = "1";

/// A DNS-SD service to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    /// Instance name.
    pub name: String,
    /// e.g. `_continuum._tcp.local.`
    pub service_type: String,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

/// The Continuum DNS-SD record. Carries only the rotating id, version and
/// flags: no host name, model or stable identifier.
pub fn continuum_record(id: ResolvableId, port: u16, flags: BeaconFlags) -> ServiceRecord {
    ServiceRecord {
        name: id.to_string(),
        service_type: MDNS_SERVICE_TYPE.to_owned(),
        port,
        txt: vec![
            (TXT_VERSION.into(), VERSION.into()),
            (TXT_FLAGS.into(), format!("{:x}", flags.bits())),
        ],
    }
}

/// Parses a Continuum instance name back into its rotating id.
pub fn continuum_id(instance: &str) -> Option<ResolvableId> {
    instance.parse().ok()
}

/// Interprets a resolved DNS-SD service as a Continuum sighting.
pub fn lan_observation(
    instance: &str,
    txt: &[(String, String)],
    addr: SocketAddr,
    at: Timestamp,
) -> Option<Observation> {
    let get = |key: &str| {
        txt.iter()
            .find_map(|(k, v)| (k == key).then_some(v.as_str()))
    };
    if get(TXT_VERSION) != Some(VERSION) {
        return None;
    }
    let id = continuum_id(instance)?;
    let flags = get(TXT_FLAGS)
        .and_then(|f| u8::from_str_radix(f, 16).ok())
        .map_or(BeaconFlags::empty(), BeaconFlags::from_bits_truncate);
    let mut obs = Observation::lan(id, addr, at);
    obs.pairing = flags.contains(BeaconFlags::PAIRING);
    Some(obs)
}

/// Interprets BLE service data under the Continuum UUID.
pub fn ble_observation(service_data: &[u8], rssi: i16, at: Timestamp) -> Option<Observation> {
    let beacon = Beacon::decode(service_data).ok()?;
    let mut obs = Observation::ble(beacon.id, rssi, at);
    obs.pairing = beacon.flags.contains(BeaconFlags::PAIRING);
    Some(obs)
}

#[cfg(test)]
mod tests {
    use continuum_core::entropy::DeterministicEntropy;
    use continuum_core::rid::random_id;

    use super::*;

    #[test]
    fn lan_record_round_trip() {
        let id = random_id(&mut DeterministicEntropy::new(1));
        let record = continuum_record(id, 4000, BeaconFlags::PAIRING);
        let addr = "192.168.0.5:4000".parse().unwrap();
        let obs = lan_observation(&record.name, &record.txt, addr, Timestamp::default()).unwrap();
        assert_eq!(obs.id, id);
        assert!(obs.pairing);
    }

    #[test]
    fn ignores_records_without_version() {
        let id = random_id(&mut DeterministicEntropy::new(1));
        let record = continuum_record(id, 4000, BeaconFlags::empty());
        let addr = "192.168.0.5:4000".parse().unwrap();
        assert!(lan_observation(&record.name, &[], addr, Timestamp::default()).is_none());
    }

    #[test]
    fn beacon_round_trip() {
        let beacon = Beacon {
            flags: BeaconFlags::LAN,
            id: random_id(&mut DeterministicEntropy::new(2)),
        };
        let obs = ble_observation(&beacon.encode(), -60, Timestamp::default()).unwrap();
        assert_eq!(obs.id, beacon.id);
        assert!(!obs.pairing);
    }
}
