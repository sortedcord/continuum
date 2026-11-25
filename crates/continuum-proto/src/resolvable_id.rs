use std::fmt;
use std::str::FromStr;

/// A rotating, privacy-preserving identifier.
///
/// `nonce` is fresh random data for each rotation, and `tag` is a truncated
/// MAC of the nonce under the advertiser's presence key. Only devices that
/// hold that key can link the identifier to the advertiser; everyone else
/// sees 12 random-looking bytes. This mirrors Bluetooth's resolvable private
/// addresses, but with a much larger nonce and tag.
///
/// Deriving and resolving identifiers lives in `continuum-core`; this type
/// is just the wire representation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResolvableId {
    pub nonce: [u8; Self::NONCE_LEN],
    pub tag: [u8; Self::TAG_LEN],
}

impl ResolvableId {
    pub const NONCE_LEN: usize = 6;
    pub const TAG_LEN: usize = 6;
    pub const LEN: usize = Self::NONCE_LEN + Self::TAG_LEN;

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0; Self::LEN];
        out[..Self::NONCE_LEN].copy_from_slice(&self.nonce);
        out[Self::NONCE_LEN..].copy_from_slice(&self.tag);
        out
    }

    pub fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        let mut nonce = [0; Self::NONCE_LEN];
        let mut tag = [0; Self::TAG_LEN];
        nonce.copy_from_slice(&bytes[..Self::NONCE_LEN]);
        tag.copy_from_slice(&bytes[Self::NONCE_LEN..]);
        Self { nonce, tag }
    }
}

/// Lowercase hex, 24 characters. Safe to use as a DNS-SD instance name.
impl fmt::Display for ResolvableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.to_bytes()))
    }
}

impl fmt::Debug for ResolvableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResolvableId({self})")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid resolvable id: expected {} hex characters", ResolvableId::LEN * 2)]
pub struct ParseResolvableIdError;

impl FromStr for ResolvableId {
    type Err = ParseResolvableIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0; Self::LEN];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| ParseResolvableIdError)?;
        Ok(Self::from_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let id = ResolvableId {
            nonce: [1, 2, 3, 4, 5, 6],
            tag: [7, 8, 9, 10, 11, 12],
        };
        assert_eq!(ResolvableId::from_bytes(id.to_bytes()), id);
    }

    #[test]
    fn string_round_trip() {
        let id = ResolvableId::from_bytes([0xab; ResolvableId::LEN]);
        let text = id.to_string();
        assert_eq!(text.len(), 24);
        assert_eq!(text.parse::<ResolvableId>(), Ok(id));
    }

    #[test]
    fn rejects_malformed_strings() {
        assert!("abc".parse::<ResolvableId>().is_err());
        assert!("zz".repeat(12).parse::<ResolvableId>().is_err());
    }
}
