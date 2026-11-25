//! Quick Share's BLE wake-up advertisement.
//!
//! Android receivers only publish their mDNS service after hearing this
//! advertisement, and Android senders broadcast it while the share sheet is
//! open. So it serves two purposes:
//!
//! * Linux, sending: advertise it to make nearby Android receivers visible.
//! * Linux, receiving: scan for it to know when to publish our own mDNS
//!   record (see [`crate::visibility`]).
//!
//! The format is from NearDrop's notes and still needs checking against
//! real devices: they only document that Android looks for this prefix,
//! not that every Android sender emits exactly these bytes.

use continuum_core::entropy::Entropy;

/// 16-bit service UUID (Google LLC) carrying the service data.
pub const SERVICE_UUID16: u16 = 0xFE2C;

/// Fixed prefix of the service data.
pub const TRIGGER_PREFIX: [u8; 14] = [
    0xfc, 0x12, 0x8e, 0x01, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

pub const TRIGGER_LEN: usize = TRIGGER_PREFIX.len() + 10;

// 16-bit service data AD structure (len, type, uuid16, data) must fit a
// legacy advertisement.
const _: () = assert!(1 + 1 + 2 + TRIGGER_LEN <= 31);

/// Service data for a wake-up advertisement: the prefix plus 10 random
/// bytes. Use fresh random bytes for each advertising session.
pub fn trigger_service_data(entropy: &mut impl Entropy) -> [u8; TRIGGER_LEN] {
    let mut out = [0; TRIGGER_LEN];
    out[..TRIGGER_PREFIX.len()].copy_from_slice(&TRIGGER_PREFIX);
    entropy.fill(&mut out[TRIGGER_PREFIX.len()..]);
    out
}

/// Whether service data under [`SERVICE_UUID16`] is a Quick Share trigger.
pub fn is_trigger(service_data: &[u8]) -> bool {
    service_data.starts_with(&TRIGGER_PREFIX)
}

#[cfg(test)]
mod tests {
    use continuum_core::entropy::DeterministicEntropy;

    use super::*;

    #[test]
    fn trigger_layout() {
        let mut entropy = DeterministicEntropy::new(1);
        let a = trigger_service_data(&mut entropy);
        let b = trigger_service_data(&mut entropy);
        assert!(is_trigger(&a) && is_trigger(&b));
        assert_ne!(a[14..], b[14..]);
    }

    #[test]
    fn rejects_other_service_data() {
        assert!(!is_trigger(&[0xfc, 0x12]));
        assert!(!is_trigger(&[0u8; TRIGGER_LEN]));
    }
}
