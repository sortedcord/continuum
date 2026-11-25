//! Linux adapters for Continuum.
//!
//! * [`avahi`]: DNS-SD publish/browse through the system Avahi daemon.
//! * [`ble`]: BLE advertising and scanning through BlueZ.
//! * [`keyring`]: identity and trust store in the Secret Service.
//! * [`discovery`]: maps Avahi/BlueZ results to core observations and Quick
//!   Share endpoints.

pub mod avahi;
pub mod ble;
pub mod discovery;
pub mod keyring;
