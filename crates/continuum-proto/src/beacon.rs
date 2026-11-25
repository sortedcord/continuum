//! BLE beacon codec.
//!
//! The beacon is the service data attached to [`crate::BLE_SERVICE_UUID`]
//! in a non-connectable legacy advertisement:
//!
//! ```text
//! AD structure: len(1) | type 0x21(1) | uuid(16) | beacon(13)  = 31 bytes
//! beacon:       version(4 bits) | flags(4 bits) | ResolvableId(12)
//! ```
//!
//! Non-connectable advertisements carry no Flags AD structure, so the 13-byte
//! beacon exactly fills the 31-byte legacy limit.

use crate::ResolvableId;

pub const BEACON_VERSION: u8 = 1;
pub const BEACON_LEN: usize = 1 + ResolvableId::LEN;

// len + AD type + 128-bit UUID + beacon must exactly fill a legacy
// advertisement.
const _: () = assert!(1 + 1 + 16 + BEACON_LEN == 31);

/// Flags in the low nibble of the beacon header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BeaconFlags(u8);

impl BeaconFlags {
    /// The device is in user-initiated pairing mode. Its identifier is random
    /// and cannot be resolved; scanners may offer it as a pairing candidate.
    pub const PAIRING: Self = Self(0b0001);
    /// The device is also reachable over the local network (mDNS).
    pub const LAN: Self = Self(0b0010);

    const MASK: u8 = 0b1111;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Keeps only the four flag bits; unknown bits are preserved for
    /// forward compatibility.
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::MASK)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Beacon {
    pub flags: BeaconFlags,
    pub id: ResolvableId,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BeaconError {
    #[error("beacon must be {BEACON_LEN} bytes, got {0}")]
    WrongLength(usize),
    #[error("unsupported beacon version {0}")]
    UnsupportedVersion(u8),
}

impl Beacon {
    pub fn encode(&self) -> [u8; BEACON_LEN] {
        let mut out = [0; BEACON_LEN];
        out[0] = (BEACON_VERSION << 4) | self.flags.bits();
        out[1..].copy_from_slice(&self.id.to_bytes());
        out
    }

    /// Decodes service data received from a scan.
    ///
    /// # Errors
    ///
    /// Fails if the payload has the wrong length or an unknown version.
    pub fn decode(data: &[u8]) -> Result<Self, BeaconError> {
        let bytes: &[u8; BEACON_LEN] = data
            .try_into()
            .map_err(|_| BeaconError::WrongLength(data.len()))?;
        let version = bytes[0] >> 4;
        if version != BEACON_VERSION {
            return Err(BeaconError::UnsupportedVersion(version));
        }
        let mut id = [0; ResolvableId::LEN];
        id.copy_from_slice(&bytes[1..]);
        Ok(Self {
            flags: BeaconFlags::from_bits_truncate(bytes[0]),
            id: ResolvableId::from_bytes(id),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Beacon {
        Beacon {
            flags: BeaconFlags::LAN,
            id: ResolvableId::from_bytes([0x5a; ResolvableId::LEN]),
        }
    }

    #[test]
    fn round_trip() {
        let beacon = sample();
        assert_eq!(Beacon::decode(&beacon.encode()), Ok(beacon));
    }

    #[test]
    fn header_layout() {
        let bytes = sample().encode();
        assert_eq!(bytes[0], 0x12);
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(Beacon::decode(&[0x10; 5]), Err(BeaconError::WrongLength(5)));
    }

    #[test]
    fn rejects_unknown_version() {
        let mut bytes = sample().encode();
        bytes[0] = 0x2f;
        assert_eq!(
            Beacon::decode(&bytes),
            Err(BeaconError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn flags_combine() {
        let flags = BeaconFlags::PAIRING.with(BeaconFlags::LAN);
        assert!(flags.contains(BeaconFlags::PAIRING));
        assert!(flags.contains(BeaconFlags::LAN));
        assert!(!BeaconFlags::empty().contains(BeaconFlags::LAN));
    }
}
