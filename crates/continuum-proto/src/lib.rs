//! Wire formats for the Continuum protocol.
//!
//! Two kinds of format live here:
//!
//! * Compact, hand-rolled binary codecs for size-constrained discovery
//!   payloads ([`beacon`], [`ResolvableId`]). A legacy BLE advertisement
//!   holds only 31 bytes, so Protobuf framing is too expensive there.
//! * Protobuf messages for everything sent inside an authenticated session
//!   ([`v1`]).

pub mod beacon;
mod resolvable_id;

pub use resolvable_id::{ParseResolvableIdError, ResolvableId};

/// Protobuf messages for protocol version 1.
#[allow(clippy::pedantic, missing_debug_implementations)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/continuum.v1.rs"));
}

/// Protobuf messages for optional features, by capability and version.
#[allow(clippy::pedantic, missing_debug_implementations)]
pub mod features {
    pub mod page {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/continuum.features.page.v1.rs"));
        }
    }
    pub mod trust {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/continuum.features.trust.v1.rs"));
        }
    }
}

/// Current Continuum protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// 128-bit BLE service UUID that every Continuum beacon carries.
///
/// The UUID is fixed so Android can keep scanning with the screen off, which
/// requires a non-empty `ScanFilter`. It reveals that *a* Continuum device is
/// nearby, but not *which* one: the beacon body only holds a
/// [`ResolvableId`].
pub const BLE_SERVICE_UUID: &str = "9b4354e1-15ec-4324-9f00-e9c5549237f7";

/// DNS-SD service type for Continuum on the local network.
pub const MDNS_SERVICE_TYPE: &str = "_continuum._tcp.local.";
