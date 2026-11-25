//! Async transport for Continuum: framing, pairing and trusted sessions over
//! any reliable byte stream (TCP today, BLE L2CAP or USB later).
//!
//! Portable across Linux and Android; platform discovery lives elsewhere.
//!
//! Every connection starts with a 6-byte preamble (`CNTM`, version, intent)
//! followed by length-prefixed frames: a big-endian `u16` length, then that
//! many bytes. Handshake messages and encrypted transport messages are each
//! one frame.

mod frame;
mod link;
mod pairing;
mod session;

pub use frame::{Intent, MAX_FRAME_LEN, read_frame, read_preamble, write_frame, write_preamble};
pub use link::{LinkClosed, SessionLink, spawn_link};
pub use pairing::{PairingMethod, pair_initiator, pair_responder};
pub use session::{SecureStream, Session, accept_session, connect_session};

use continuum_core::pairing::PairingError;
use continuum_core::session::SessionError;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Pairing(#[from] PairingError),
    #[error("could not decode message: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("peer is not trusted")]
    UnknownPeer,
    #[error("pairing was rejected")]
    Rejected,
}
