use std::future::Future;

use continuum_core::clock::Clock;
use continuum_core::identity::DeviceIdentity;
use continuum_core::pairing::{make_record, verify_record};
use continuum_core::proto::v1::{DeviceInfo, PairingRecord, PairingResult};
use continuum_core::session::{Handshake, HandshakeKind, Role, Sas};
use continuum_core::trust::TrustedPeer;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::NetError;
use crate::frame::{Intent, write_preamble};
use crate::session::{SecureStream, drive};

#[derive(Clone, Copy)]
pub enum PairingMethod<'a> {
    /// The one-time secret from the inviter's QR code.
    Secret(&'a [u8; 32]),
    /// Users compare a six-digit code on both screens.
    Sas,
}

impl std::fmt::Debug for PairingMethod<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Secret(_) => f.write_str("Secret(<redacted>)"),
            Self::Sas => f.write_str("Sas"),
        }
    }
}

impl PairingMethod<'_> {
    fn kind(&self) -> HandshakeKind<'_> {
        match self {
            Self::Secret(secret) => HandshakeKind::PairWithSecret(secret),
            Self::Sas => HandshakeKind::PairWithSas,
        }
    }

    fn intent(self) -> Intent {
        match self {
            Self::Secret(_) => Intent::PairWithSecret,
            Self::Sas => Intent::PairWithSas,
        }
    }
}

/// Shared tail of both roles: swap signed records, confirm, swap results.
async fn finish<S, F, Fut>(
    mut stream: SecureStream<S>,
    identity: &DeviceIdentity,
    device: DeviceInfo,
    method: PairingMethod<'_>,
    confirm: F,
    clock: &dyn Clock,
    initiator: bool,
) -> Result<TrustedPeer, NetError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(Sas, TrustedPeer) -> Fut,
    Fut: Future<Output = bool>,
{
    let ours = make_record(identity, device, stream.channel());
    let theirs: PairingRecord = if initiator {
        stream.send(&ours).await?;
        stream.recv().await?
    } else {
        let theirs = stream.recv().await?;
        stream.send(&ours).await?;
        theirs
    };
    let peer = verify_record(&theirs, stream.channel(), clock.now())?;

    let accepted = match method {
        PairingMethod::Secret(_) => true,
        PairingMethod::Sas => confirm(stream.channel().sas(), peer.clone()).await,
    };
    stream.send(&PairingResult { accepted }).await?;
    let result: PairingResult = stream.recv().await?;
    if accepted && result.accepted {
        Ok(peer)
    } else {
        Err(NetError::Rejected)
    }
}

/// Joins a pairing: the side that scanned the QR code (or typed the
/// address) and connects. Returns the peer to store in the trust store.
///
/// `confirm` is only called for [`PairingMethod::Sas`]; it should show the
/// code and resolve to whether the user confirmed it matches.
///
/// # Errors
/// Fails on a wrong secret, a bad record, rejection, or I/O errors.
pub async fn pair_initiator<S, F, Fut>(
    mut stream: S,
    identity: &DeviceIdentity,
    device: DeviceInfo,
    method: PairingMethod<'_>,
    confirm: F,
    clock: &dyn Clock,
) -> Result<TrustedPeer, NetError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(Sas, TrustedPeer) -> Fut,
    Fut: Future<Output = bool>,
{
    write_preamble(&mut stream, method.intent()).await?;
    let handshake = Handshake::new(identity, method.kind(), Role::Initiator)?;
    let channel = drive(&mut stream, handshake, |_| Ok(())).await?;
    finish(
        SecureStream::new(stream, channel),
        identity,
        device,
        method,
        confirm,
        clock,
        true,
    )
    .await
}

/// Accepts a pairing after [`crate::read_preamble`] returned a pairing
/// intent. The caller must check that the intent matches the method it is
/// offering (only accept `PairWithSecret` while an invite is live).
///
/// # Errors
/// Fails on a wrong secret, a bad record, rejection, or I/O errors.
pub async fn pair_responder<S, F, Fut>(
    mut stream: S,
    identity: &DeviceIdentity,
    device: DeviceInfo,
    method: PairingMethod<'_>,
    confirm: F,
    clock: &dyn Clock,
) -> Result<TrustedPeer, NetError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(Sas, TrustedPeer) -> Fut,
    Fut: Future<Output = bool>,
{
    let handshake = Handshake::new(identity, method.kind(), Role::Responder)?;
    let channel = drive(&mut stream, handshake, |_| Ok(())).await?;
    finish(
        SecureStream::new(stream, channel),
        identity,
        device,
        method,
        confirm,
        clock,
        false,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use continuum_core::clock::{ManualClock, Timestamp};
    use continuum_core::entropy::DeterministicEntropy;

    use super::*;
    use crate::frame::read_preamble;

    fn identity(seed: u64) -> DeviceIdentity {
        DeviceIdentity::generate(&mut DeterministicEntropy::new(seed))
    }

    fn info(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            ..Default::default()
        }
    }

    async fn run(
        joiner_method: PairingMethod<'_>,
        inviter_method: PairingMethod<'_>,
        joiner_ok: bool,
        inviter_ok: bool,
    ) -> (
        Result<TrustedPeer, NetError>,
        Result<TrustedPeer, NetError>,
        Vec<Sas>,
    ) {
        let (phone, laptop) = (identity(1), identity(2));
        let clock = ManualClock::new(Timestamp::from_millis(77));
        let (a, mut b) = tokio::io::duplex(1 << 16);
        let codes = Arc::new(Mutex::new(Vec::new()));
        let (c1, c2) = (codes.clone(), codes.clone());

        let joiner = pair_initiator(
            a,
            &phone,
            info("Phone"),
            joiner_method,
            |sas, _| {
                c1.lock().unwrap().push(sas);
                async move { joiner_ok }
            },
            &clock,
        );
        let inviter = async {
            read_preamble(&mut b).await?;
            pair_responder(
                b,
                &laptop,
                info("Laptop"),
                inviter_method,
                |sas, _| {
                    c2.lock().unwrap().push(sas);
                    async move { inviter_ok }
                },
                &clock,
            )
            .await
        };
        let (j, i) = tokio::join!(joiner, inviter);
        let codes = codes.lock().unwrap().clone();
        (j, i, codes)
    }

    #[tokio::test]
    async fn qr_secret_pairing() {
        let secret = [5; 32];
        let method = PairingMethod::Secret(&secret);
        let (joiner, inviter, codes) = run(method, method, false, false).await;
        let (laptop_seen_by_phone, phone_seen_by_laptop) = (joiner.unwrap(), inviter.unwrap());
        assert_eq!(laptop_seen_by_phone.device_id(), identity(2).device_id());
        assert_eq!(laptop_seen_by_phone.name, "Laptop");
        assert_eq!(phone_seen_by_laptop.device_id(), identity(1).device_id());
        assert_eq!(phone_seen_by_laptop.paired_at, Timestamp::from_millis(77));
        assert!(codes.is_empty(), "no SAS prompt with a QR secret");
    }

    #[tokio::test]
    async fn wrong_qr_secret_fails() {
        let (good, bad) = ([5; 32], [6; 32]);
        let (joiner, inviter, _) = run(
            PairingMethod::Secret(&bad),
            PairingMethod::Secret(&good),
            true,
            true,
        )
        .await;
        assert!(joiner.is_err() && inviter.is_err());
    }

    #[tokio::test]
    async fn sas_pairing_shows_same_code() {
        let (joiner, inviter, codes) =
            run(PairingMethod::Sas, PairingMethod::Sas, true, true).await;
        assert!(joiner.is_ok() && inviter.is_ok());
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0], codes[1]);
    }

    #[tokio::test]
    async fn sas_rejected_by_either_side_fails_both() {
        let (joiner, inviter, _) = run(PairingMethod::Sas, PairingMethod::Sas, true, false).await;
        assert!(matches!(joiner, Err(NetError::Rejected)));
        assert!(matches!(inviter, Err(NetError::Rejected)));
    }
}
