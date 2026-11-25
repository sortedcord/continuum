//! Maps raw Avahi and BlueZ results onto Continuum observations and Quick
//! Share endpoints.

use continuum_core::clock::Timestamp;
use continuum_core::presence::Observation;
use continuum_core::proto::beacon::{Beacon, BeaconFlags};
use continuum_core::proto::{MDNS_SERVICE_TYPE, ResolvableId};
use continuum_quickshare::mdns::{self as qs_mdns, QuickShareEndpoint};
use continuum_quickshare::{EndpointId, EndpointInfo};

use crate::avahi::{ResolvedService, ServiceSpec};
use crate::ble::ServiceDataSeen;

const TXT_VERSION: &str = "v";
const TXT_FLAGS: &str = "f";
const VERSION: &str = "1";

/// The Continuum DNS-SD record. Carries only the rotating id, version and
/// flags: no host name, model or stable identifier.
pub fn continuum_spec(id: ResolvableId, port: u16, flags: BeaconFlags) -> ServiceSpec {
    ServiceSpec {
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

pub fn continuum_observation(service: &ResolvedService, at: Timestamp) -> Option<Observation> {
    if service.txt(TXT_VERSION) != Some(VERSION) {
        return None;
    }
    let id = continuum_id(&service.name)?;
    let flags = service
        .txt(TXT_FLAGS)
        .and_then(|f| u8::from_str_radix(f, 16).ok())
        .map_or(BeaconFlags::empty(), BeaconFlags::from_bits_truncate);
    let mut obs = Observation::lan(id, service.addr, at);
    obs.pairing = flags.contains(BeaconFlags::PAIRING);
    Some(obs)
}

pub fn beacon_observation(seen: &ServiceDataSeen, at: Timestamp) -> Option<Observation> {
    let beacon = Beacon::decode(&seen.data).ok()?;
    let mut obs = Observation::ble(beacon.id, seen.rssi?, at);
    obs.pairing = beacon.flags.contains(BeaconFlags::PAIRING);
    Some(obs)
}

/// The Quick Share receiver record.
///
/// # Errors
/// Fails if the endpoint info can't be encoded.
pub fn quickshare_spec(
    id: &EndpointId,
    info: &EndpointInfo,
    port: u16,
) -> Result<ServiceSpec, qs_mdns::MdnsError> {
    Ok(ServiceSpec {
        name: qs_mdns::instance_name(id),
        service_type: qs_mdns::SERVICE_TYPE.to_owned(),
        port,
        txt: vec![(qs_mdns::TXT_KEY.into(), qs_mdns::encode_txt(info)?)],
    })
}

pub fn quickshare_endpoint(service: &ResolvedService) -> Option<QuickShareEndpoint> {
    let txt = service.txt.iter().map(|(k, v)| (k.as_str(), v.as_str()));
    QuickShareEndpoint::from_service(&service.name, txt, service.addr).ok()
}

#[cfg(test)]
mod tests {
    use continuum_core::entropy::DeterministicEntropy;
    use continuum_core::rid::random_id;

    use super::*;

    fn resolved(spec: &ServiceSpec) -> ResolvedService {
        ResolvedService {
            name: spec.name.clone(),
            host: "host.local".into(),
            addr: "192.168.0.5:4000".parse().unwrap(),
            txt: spec.txt.clone(),
        }
    }

    #[test]
    fn continuum_record_round_trip() {
        let id = random_id(&mut DeterministicEntropy::new(1));
        let spec = continuum_spec(id, 4000, BeaconFlags::PAIRING);
        let obs = continuum_observation(&resolved(&spec), Timestamp::default()).unwrap();
        assert_eq!(obs.id, id);
        assert!(obs.pairing);
        assert_eq!(obs.addr, Some("192.168.0.5:4000".parse().unwrap()));
    }

    #[test]
    fn ignores_records_without_version() {
        let id = random_id(&mut DeterministicEntropy::new(1));
        let mut spec = continuum_spec(id, 4000, BeaconFlags::empty());
        spec.txt.retain(|(k, _)| k != TXT_VERSION);
        assert!(continuum_observation(&resolved(&spec), Timestamp::default()).is_none());
    }

    #[test]
    fn beacon_needs_rssi() {
        let beacon = Beacon {
            flags: BeaconFlags::empty(),
            id: random_id(&mut DeterministicEntropy::new(2)),
        };
        let mut seen = ServiceDataSeen {
            address: bluer::Address::any(),
            uuid: bluer::Uuid::nil(),
            data: beacon.encode().to_vec(),
            rssi: Some(-60),
        };
        assert_eq!(
            beacon_observation(&seen, Timestamp::default()).unwrap().id,
            beacon.id
        );
        seen.rssi = None;
        assert!(beacon_observation(&seen, Timestamp::default()).is_none());
    }

    #[test]
    fn quickshare_record_round_trip() {
        let mut entropy = DeterministicEntropy::new(3);
        let id = EndpointId::generate(&mut entropy);
        let info = EndpointInfo::visible(
            "motobook",
            continuum_quickshare::DeviceType::Laptop,
            &mut entropy,
        );
        let spec = quickshare_spec(&id, &info, 4000).unwrap();
        let endpoint = quickshare_endpoint(&resolved(&spec)).unwrap();
        assert_eq!(endpoint.id, id);
        assert_eq!(endpoint.info, info);
    }
}
