//! Maps Avahi results onto Quick Share endpoints. Continuum's own records
//! live in [`continuum_node::records`].

use continuum_quickshare::mdns::{self as qs_mdns, QuickShareEndpoint};
use continuum_quickshare::{EndpointId, EndpointInfo};

use crate::avahi::{ResolvedService, ServiceSpec};

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

    use super::*;

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
        let resolved = ResolvedService {
            name: spec.name.clone(),
            host: "host.local".into(),
            addr: "192.168.0.5:4000".parse().unwrap(),
            txt: spec.txt.clone(),
        };
        let endpoint = quickshare_endpoint(&resolved).unwrap();
        assert_eq!(endpoint.id, id);
        assert_eq!(endpoint.info, info);
    }
}
