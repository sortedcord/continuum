//! Portable core of Continuum.
//!
//! This crate does no I/O. Platform adapters (BlueZ, Avahi, Android) feed it
//! observations and timestamps, and it answers the three questions every
//! Continuity feature depends on:
//!
//! 1. *Who is nearby?* ([`presence`])
//! 2. *Is it a trusted device?* ([`identity`], [`trust`], [`rid`])
//! 3. *How do I reach it?* ([`presence::PeerPresence`])
//!
//! Everything is deterministic given a [`clock::Clock`] and an
//! [`entropy::Entropy`] source, so it can be tested without radios
//! (see [`sim`]).

pub mod clock;
pub mod entropy;
pub mod identity;
pub mod presence;
pub mod rid;
pub mod sim;
pub mod trust;

pub use continuum_proto as proto;
