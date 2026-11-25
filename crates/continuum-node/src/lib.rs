//! A portable Continuum node.
//!
//! The node owns everything that is the same on every platform: the rotating
//! identifier, the presence registry, the TCP listener and one encrypted
//! session per paired peer. Platform code only has to:
//!
//! * feed it discovery results ([`NodeHandle::observe_lan`],
//!   [`NodeHandle::observe_ble`], ...), and
//! * act on [`NodeEvent::Advertise`] by (re)publishing the mDNS record and
//!   BLE beacon it describes.
//!
//! The Linux daemon drives it with Avahi and BlueZ; the Android app drives it
//! through `continuum-ffi` with NsdManager and the BLE APIs.

mod addrs;
mod node;
pub mod pairing;
pub mod records;

pub use addrs::{connect_any, rank_addrs};
pub use node::{Advertisement, NodeConfig, NodeEvent, NodeHandle, PeerStatus, start};

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Net(#[from] continuum_net::NetError),
    #[error(transparent)]
    Pairing(#[from] continuum_core::pairing::PairingError),
    #[error("none of {0} address(es) answered")]
    Unreachable(usize),
    #[error("timed out")]
    TimedOut,
    #[error("the node has stopped")]
    Stopped,
}
