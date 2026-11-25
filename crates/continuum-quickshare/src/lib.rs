//! Discovery-level compatibility with Google/Samsung Quick Share.
//!
//! Quick Share is a foreign protocol: its devices are never Continuum trust
//! peers, and nothing here touches the Continuum trust store. This crate
//! only lets a Linux machine
//!
//! * show up as a receiver in Android's share sheet ([`mdns`],
//!   [`visibility`]), and
//! * make nearby Android receivers publish themselves so Linux can find
//!   them ([`ble`]).
//!
//! The UKEY2 handshake and file transfer belong to Checkpoint 3.
//!
//! Formats follow grishka's NearDrop protocol notes (Unlicense),
//! <https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md>.
//!
//! Privacy note: the endpoint info carries the device name in plain text,
//! which goes against Continuum's own discovery rules. Quick Share
//! visibility should therefore be opt-in and, by default, limited to
//! windows when an Android sender is actually nearby.

pub mod ble;
pub mod endpoint;
pub mod mdns;
pub mod visibility;

pub use endpoint::{DeviceType, EndpointId, EndpointInfo};
