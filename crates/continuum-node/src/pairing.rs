//! Joining a pairing from the connecting side.

use std::future::Future;
use std::net::SocketAddr;

use continuum_core::clock::Clock;
use continuum_core::identity::DeviceIdentity;
use continuum_core::pairing::Invite;
use continuum_core::proto::v1::DeviceInfo;
use continuum_core::session::Sas;
use continuum_core::trust::TrustedPeer;
use continuum_net::{PairingMethod, pair_initiator};

use crate::{NodeError, connect_any, rank_addrs};

/// Pairs using an invite (the text in the other device's QR code).
///
/// # Errors
/// Fails if the invite is invalid or expired, or pairing fails.
pub async fn join_invite(
    identity: &DeviceIdentity,
    device: DeviceInfo,
    invite: &str,
    clock: &dyn Clock,
) -> Result<TrustedPeer, NodeError> {
    let invite = Invite::decode(invite)?;
    if invite.is_expired(clock.now()) {
        return Err(continuum_core::pairing::PairingError::Expired.into());
    }
    let stream = connect_any(&rank_addrs(invite.addrs.iter().copied())).await?;
    let never = |_, _| async { false };
    Ok(pair_initiator(
        stream,
        identity,
        device,
        PairingMethod::Secret(&invite.secret),
        never,
        clock,
    )
    .await?)
}

/// Pairs by code comparison with a device in pairing mode at `addrs`.
///
/// # Errors
/// Fails if the device is unreachable, either user rejects, or pairing fails.
pub async fn join_with_code<F, Fut>(
    identity: &DeviceIdentity,
    device: DeviceInfo,
    addrs: &[SocketAddr],
    confirm: F,
    clock: &dyn Clock,
) -> Result<TrustedPeer, NodeError>
where
    F: FnOnce(Sas, TrustedPeer) -> Fut,
    Fut: Future<Output = bool>,
{
    let stream = connect_any(&rank_addrs(addrs.iter().copied())).await?;
    Ok(pair_initiator(stream, identity, device, PairingMethod::Sas, confirm, clock).await?)
}
