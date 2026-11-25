//! Quick Share's DNS-SD records.
//!
//! A receiver publishes `<instance>._FC9F5ED42C8A._tcp.local.` with a TXT
//! record `n=<endpoint info>`. Both the instance name and the TXT value are
//! URL-safe base64 without padding.

use std::net::SocketAddr;

use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};

use crate::endpoint::{EndpointId, EndpointInfo, EndpointInfoError};

/// DNS-SD service type. `FC9F5ED42C8A` is the start of
/// `SHA-256("NearbySharing")`.
pub const SERVICE_TYPE: &str = "_FC9F5ED42C8A._tcp.local.";
/// TXT record key holding the endpoint info.
pub const TXT_KEY: &str = "n";

const PCP: u8 = 0x23;
const SERVICE_ID_HASH: [u8; 3] = [0xFC, 0x9F, 0x5E];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MdnsError {
    #[error("not valid URL-safe base64")]
    Base64,
    #[error("instance name is not a Quick Share endpoint")]
    NotQuickShare,
    #[error("missing `n` TXT record")]
    MissingEndpointInfo,
    #[error(transparent)]
    EndpointInfo(#[from] EndpointInfoError),
}

fn decode_base64(text: &str) -> Result<Vec<u8>, MdnsError> {
    // Unpadded is what Android and NearDrop send; accept padding too.
    URL_SAFE_NO_PAD
        .decode(text)
        .or_else(|_| URL_SAFE.decode(text))
        .map_err(|_| MdnsError::Base64)
}

/// Builds the DNS-SD instance name for an endpoint.
pub fn instance_name(id: &EndpointId) -> String {
    let mut raw = [0u8; 10];
    raw[0] = PCP;
    raw[1..5].copy_from_slice(id.as_bytes());
    raw[5..8].copy_from_slice(&SERVICE_ID_HASH);
    URL_SAFE_NO_PAD.encode(raw)
}

/// Extracts the endpoint ID from an instance name.
///
/// # Errors
///
/// Fails if the name is not base64 or not a Quick Share endpoint.
pub fn parse_instance_name(name: &str) -> Result<EndpointId, MdnsError> {
    let raw = decode_base64(name)?;
    let [PCP, a, b, c, d, 0xFC, 0x9F, 0x5E, ..] = raw[..] else {
        return Err(MdnsError::NotQuickShare);
    };
    if raw.len() < 10 {
        return Err(MdnsError::NotQuickShare);
    }
    EndpointId::from_bytes([a, b, c, d]).ok_or(MdnsError::NotQuickShare)
}

/// Encodes endpoint info for the `n` TXT record.
///
/// # Errors
///
/// Fails if the endpoint info cannot be encoded.
pub fn encode_txt(info: &EndpointInfo) -> Result<String, MdnsError> {
    Ok(URL_SAFE_NO_PAD.encode(info.encode()?))
}

/// # Errors
///
/// Fails if the value is not base64 or not valid endpoint info.
pub fn decode_txt(value: &str) -> Result<EndpointInfo, MdnsError> {
    Ok(EndpointInfo::decode(&decode_base64(value)?)?)
}

/// A Quick Share receiver found on the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickShareEndpoint {
    pub id: EndpointId,
    pub info: EndpointInfo,
    pub addr: SocketAddr,
}

impl QuickShareEndpoint {
    /// Interprets a resolved DNS-SD service.
    ///
    /// # Errors
    ///
    /// Fails if the instance name or TXT record is not Quick Share's.
    pub fn from_service<'a>(
        instance: &str,
        txt: impl IntoIterator<Item = (&'a str, &'a str)>,
        addr: SocketAddr,
    ) -> Result<Self, MdnsError> {
        let id = parse_instance_name(instance)?;
        let value = txt
            .into_iter()
            .find_map(|(key, value)| (key == TXT_KEY).then_some(value))
            .ok_or(MdnsError::MissingEndpointInfo)?;
        Ok(Self {
            id,
            info: decode_txt(value)?,
            addr,
        })
    }
}

#[cfg(test)]
mod tests {
    use continuum_core::entropy::DeterministicEntropy;

    use super::*;
    use crate::endpoint::DeviceType;

    fn id() -> EndpointId {
        EndpointId::from_bytes(*b"Ab12").unwrap()
    }

    #[test]
    fn instance_name_layout() {
        let name = instance_name(&id());
        let raw = URL_SAFE_NO_PAD.decode(&name).unwrap();
        assert_eq!(raw, [0x23, b'A', b'b', b'1', b'2', 0xFC, 0x9F, 0x5E, 0, 0]);
        assert!(!name.contains('='));
        assert_eq!(parse_instance_name(&name), Ok(id()));
    }

    #[test]
    fn accepts_padded_instance_names() {
        let padded = URL_SAFE.encode([0x23, b'A', b'b', b'1', b'2', 0xFC, 0x9F, 0x5E, 0, 0]);
        assert!(padded.ends_with('='));
        assert_eq!(parse_instance_name(&padded), Ok(id()));
    }

    #[test]
    fn rejects_foreign_instance_names() {
        assert_eq!(parse_instance_name("my-printer"), Err(MdnsError::Base64));
        let other = URL_SAFE_NO_PAD.encode([0x23, b'A', b'b', b'1', b'2', 1, 2, 3, 0, 0]);
        assert_eq!(parse_instance_name(&other), Err(MdnsError::NotQuickShare));
    }

    #[test]
    fn endpoint_from_service() {
        let info = EndpointInfo::visible(
            "Pixel 9",
            DeviceType::Phone,
            &mut DeterministicEntropy::new(1),
        );
        let txt = encode_txt(&info).unwrap();
        let addr: SocketAddr = "192.168.1.40:41234".parse().unwrap();
        let endpoint =
            QuickShareEndpoint::from_service(&instance_name(&id()), [("n", txt.as_str())], addr)
                .unwrap();
        assert_eq!(endpoint.id, id());
        assert_eq!(endpoint.info, info);

        assert_eq!(
            QuickShareEndpoint::from_service(&instance_name(&id()), [], addr),
            Err(MdnsError::MissingEndpointInfo)
        );
    }
}
