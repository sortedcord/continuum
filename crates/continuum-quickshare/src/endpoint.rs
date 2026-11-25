//! Endpoint identifiers and the "endpoint info" blob.

use std::fmt;

use continuum_core::entropy::Entropy;

/// Four random ASCII alphanumerics that name a Quick Share endpoint for the
/// lifetime of an advertisement.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EndpointId([u8; 4]);

const ALPHABET: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

impl EndpointId {
    pub fn generate(entropy: &mut impl Entropy) -> Self {
        let mut out = [0; 4];
        for slot in &mut out {
            // Rejection sampling keeps the distribution uniform.
            *slot = loop {
                let [byte] = entropy.array::<1>();
                if usize::from(byte) < 248 {
                    break ALPHABET[usize::from(byte) % ALPHABET.len()];
                }
            };
        }
        Self(out)
    }

    /// Accepts any four ASCII alphanumerics.
    pub fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        bytes
            .iter()
            .all(u8::is_ascii_alphanumeric)
            .then_some(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Always ASCII alphanumeric, see constructors.
        f.write_str(std::str::from_utf8(&self.0).unwrap_or("????"))
    }
}

impl fmt::Debug for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EndpointId({self})")
    }
}

/// Icon hint shown by Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Unknown,
    Phone,
    Tablet,
    Laptop,
    Car,
    Foldable,
    Xr,
    Other(u8),
}

impl DeviceType {
    fn to_bits(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Phone => 1,
            Self::Tablet => 2,
            Self::Laptop => 3,
            Self::Car => 4,
            Self::Foldable => 5,
            Self::Xr => 6,
            Self::Other(bits) => bits & 0b111,
        }
    }

    fn from_bits(bits: u8) -> Self {
        match bits & 0b111 {
            0 => Self::Unknown,
            1 => Self::Phone,
            2 => Self::Tablet,
            3 => Self::Laptop,
            4 => Self::Car,
            5 => Self::Foldable,
            6 => Self::Xr,
            other => Self::Other(other),
        }
    }
}

const TLV_QR_CODE: u8 = 1;
const TLV_VENDOR_ID: u8 = 2;
const METADATA_LEN: usize = 16;

/// Endpoint info, sent in the mDNS `n` TXT record and in connection
/// requests.
///
/// ```text
/// byte 0:    version(3 bits) | hidden(1) | device type(3) | reserved(1)
/// 1..17:     salt(2) + encrypted metadata key(14); random is fine offline
/// if !hidden: name_len(1) + UTF-8 name
/// then:      TLV records: type(1) | len(1) | value
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    pub version: u8,
    pub device_type: DeviceType,
    /// `None` means the device is hidden (visibility bit set).
    pub name: Option<String>,
    pub metadata: [u8; METADATA_LEN],
    /// TLV type 1: advertising token or encrypted name, for QR-code flows.
    pub qr_code: Option<Vec<u8>>,
    /// TLV type 2: 0 = none, 1 = Samsung.
    pub vendor_id: Option<u8>,
    /// Unrecognised TLV records, preserved for round-tripping.
    pub extra: Vec<(u8, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointInfoError {
    #[error("endpoint info is truncated")]
    Truncated,
    #[error("device name is not valid UTF-8")]
    InvalidName,
    #[error("TLV value for type {0} is longer than 255 bytes")]
    TlvTooLong(u8),
}

impl EndpointInfo {
    /// A visible endpoint with random metadata, as a Linux receiver would
    /// advertise it.
    pub fn visible(name: &str, device_type: DeviceType, entropy: &mut impl Entropy) -> Self {
        Self {
            version: 0,
            device_type,
            name: Some(name.to_owned()),
            metadata: entropy.array(),
            qr_code: None,
            vendor_id: None,
            extra: Vec::new(),
        }
    }

    /// # Errors
    ///
    /// Fails if a TLV value does not fit its one-byte length.
    pub fn encode(&self) -> Result<Vec<u8>, EndpointInfoError> {
        let hidden = self.name.is_none();
        let mut out = vec![
            ((self.version & 0b111) << 5)
                | (u8::from(hidden) << 4)
                | (self.device_type.to_bits() << 1),
        ];
        out.extend_from_slice(&self.metadata);
        if let Some(name) = &self.name {
            let name = truncate_utf8(name, 255);
            out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(name.as_bytes());
        }
        let tlvs = self
            .qr_code
            .iter()
            .map(|value| (TLV_QR_CODE, value.as_slice()))
            .chain(
                self.vendor_id
                    .as_ref()
                    .map(|vendor| (TLV_VENDOR_ID, std::slice::from_ref(vendor))),
            )
            .chain(
                self.extra
                    .iter()
                    .map(|(kind, value)| (*kind, value.as_slice())),
            );
        for (kind, value) in tlvs {
            let len = u8::try_from(value.len()).map_err(|_| EndpointInfoError::TlvTooLong(kind))?;
            out.extend_from_slice(&[kind, len]);
            out.extend_from_slice(value);
        }
        Ok(out)
    }

    /// Parses endpoint info. Trailing bytes that do not form a complete TLV
    /// record are ignored, as Android does.
    ///
    /// # Errors
    ///
    /// Fails if the fixed header or the name is truncated, or the name is
    /// not UTF-8.
    pub fn decode(data: &[u8]) -> Result<Self, EndpointInfoError> {
        let (&flags, rest) = data.split_first().ok_or(EndpointInfoError::Truncated)?;
        let (metadata, mut rest) = rest
            .split_first_chunk::<METADATA_LEN>()
            .ok_or(EndpointInfoError::Truncated)?;
        let hidden = flags & 0b0001_0000 != 0;

        let name = if hidden {
            None
        } else {
            let (&len, tail) = rest.split_first().ok_or(EndpointInfoError::Truncated)?;
            let (name, tail) = tail
                .split_at_checked(usize::from(len))
                .ok_or(EndpointInfoError::Truncated)?;
            rest = tail;
            Some(
                std::str::from_utf8(name)
                    .map_err(|_| EndpointInfoError::InvalidName)?
                    .to_owned(),
            )
        };

        let mut info = Self {
            version: flags >> 5,
            device_type: DeviceType::from_bits(flags >> 1),
            name,
            metadata: *metadata,
            qr_code: None,
            vendor_id: None,
            extra: Vec::new(),
        };
        while let [kind, len, tail @ ..] = rest {
            let Some((value, tail)) = tail.split_at_checked(usize::from(*len)) else {
                break;
            };
            match (*kind, value) {
                (TLV_QR_CODE, _) => info.qr_code = Some(value.to_vec()),
                (TLV_VENDOR_ID, [vendor]) => info.vendor_id = Some(*vendor),
                _ => info.extra.push((*kind, value.to_vec())),
            }
            rest = tail;
        }
        Ok(info)
    }
}

fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use continuum_core::entropy::DeterministicEntropy;

    use super::*;

    #[test]
    fn endpoint_ids_are_alphanumeric() {
        let mut entropy = DeterministicEntropy::new(1);
        for _ in 0..200 {
            let id = EndpointId::generate(&mut entropy);
            assert!(id.as_bytes().iter().all(u8::is_ascii_alphanumeric));
        }
        assert!(EndpointId::from_bytes(*b"ab-d").is_none());
    }

    #[test]
    fn visible_round_trip() {
        let mut entropy = DeterministicEntropy::new(2);
        let info = EndpointInfo::visible("Workstation", DeviceType::Laptop, &mut entropy);
        let bytes = info.encode().unwrap();
        assert_eq!(bytes[0], 0b0000_0110, "laptop, visible, version 0");
        assert_eq!(EndpointInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn hidden_with_tlvs_round_trip() {
        let info = EndpointInfo {
            version: 1,
            device_type: DeviceType::Phone,
            name: None,
            metadata: [7; 16],
            qr_code: Some(vec![1, 2, 3]),
            vendor_id: Some(1),
            extra: vec![(9, vec![0xaa])],
        };
        let bytes = info.encode().unwrap();
        assert_eq!(bytes[0], 0b0011_0010);
        assert_eq!(EndpointInfo::decode(&bytes).unwrap(), info);
    }

    #[test]
    fn parses_hand_built_android_style_blob() {
        // Phone named "Pixel" with a Samsung vendor TLV.
        let mut blob = vec![0b0000_0010];
        blob.extend([0u8; 16]);
        blob.push(5);
        blob.extend(b"Pixel");
        blob.extend([2, 1, 1]);
        let info = EndpointInfo::decode(&blob).unwrap();
        assert_eq!(info.device_type, DeviceType::Phone);
        assert_eq!(info.name.as_deref(), Some("Pixel"));
        assert_eq!(info.vendor_id, Some(1));
    }

    #[test]
    fn ignores_incomplete_trailing_tlv() {
        let mut blob =
            EndpointInfo::visible("x", DeviceType::Unknown, &mut DeterministicEntropy::new(3))
                .encode()
                .unwrap();
        blob.extend([1, 10, 0]);
        assert_eq!(EndpointInfo::decode(&blob).unwrap().qr_code, None);
    }

    #[test]
    fn rejects_truncated_input() {
        assert_eq!(EndpointInfo::decode(&[]), Err(EndpointInfoError::Truncated));
        assert_eq!(
            EndpointInfo::decode(&[0; 10]),
            Err(EndpointInfoError::Truncated)
        );
        let mut blob = vec![0];
        blob.extend([0; 16]);
        blob.push(20);
        blob.extend(b"short");
        assert_eq!(
            EndpointInfo::decode(&blob),
            Err(EndpointInfoError::Truncated)
        );
    }

    #[test]
    fn long_names_truncate_on_char_boundary() {
        let name = "é".repeat(200); // 400 bytes
        let mut info =
            EndpointInfo::visible(&name, DeviceType::Laptop, &mut DeterministicEntropy::new(4));
        let decoded = EndpointInfo::decode(&info.encode().unwrap()).unwrap();
        assert_eq!(decoded.name.as_ref().unwrap().len(), 254);
        info.qr_code = Some(vec![0; 300]);
        assert_eq!(info.encode(), Err(EndpointInfoError::TlvTooLong(1)));
    }
}
