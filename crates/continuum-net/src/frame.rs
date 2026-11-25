use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::NetError;

const MAGIC: &[u8; 4] = b"CNTM";
const VERSION: u8 = 1;

/// Largest frame body.
pub const MAX_FRAME_LEN: usize = u16::MAX as usize;

/// What the connecting side wants, announced in the preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Noise `IK` session between paired devices.
    Session,
    /// Pairing with the QR code secret (`XXpsk3`).
    PairWithSecret,
    /// Pairing with SAS comparison (`XX`).
    PairWithSas,
}

impl Intent {
    const fn to_byte(self) -> u8 {
        match self {
            Self::Session => 1,
            Self::PairWithSecret => 2,
            Self::PairWithSas => 3,
        }
    }

    const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Session),
            2 => Some(Self::PairWithSecret),
            3 => Some(Self::PairWithSas),
            _ => None,
        }
    }
}

/// # Errors
/// Fails on I/O errors.
pub async fn write_preamble<W: AsyncWrite + Unpin>(
    w: &mut W,
    intent: Intent,
) -> Result<(), NetError> {
    let mut preamble = [0; 6];
    preamble[..4].copy_from_slice(MAGIC);
    preamble[4] = VERSION;
    preamble[5] = intent.to_byte();
    w.write_all(&preamble).await?;
    w.flush().await?;
    Ok(())
}

/// # Errors
/// Fails on I/O errors or if the peer is not speaking Continuum v1.
pub async fn read_preamble<R: AsyncRead + Unpin>(r: &mut R) -> Result<Intent, NetError> {
    let mut preamble = [0; 6];
    r.read_exact(&mut preamble).await?;
    if &preamble[..4] != MAGIC {
        return Err(NetError::Protocol("not a Continuum connection"));
    }
    if preamble[4] != VERSION {
        return Err(NetError::Protocol("unsupported protocol version"));
    }
    Intent::from_byte(preamble[5]).ok_or(NetError::Protocol("unknown intent"))
}

/// # Errors
/// Fails on I/O errors or if `data` exceeds [`MAX_FRAME_LEN`].
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> Result<(), NetError> {
    let len = u16::try_from(data.len()).map_err(|_| NetError::Protocol("frame too large"))?;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(data).await?;
    w.flush().await?;
    Ok(())
}

/// # Errors
/// Fails on I/O errors, including a clean EOF.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, NetError> {
    let len = r.read_u16().await?;
    let mut data = vec![0; usize::from(len)];
    r.read_exact(&mut data).await?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1 << 17);
        write_frame(&mut a, b"hello").await.unwrap();
        write_frame(&mut a, &[]).await.unwrap();
        write_frame(&mut a, &vec![7; MAX_FRAME_LEN]).await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap(), b"hello");
        assert_eq!(read_frame(&mut b).await.unwrap(), b"");
        assert_eq!(read_frame(&mut b).await.unwrap().len(), MAX_FRAME_LEN);
        assert!(
            write_frame(&mut a, &vec![0; MAX_FRAME_LEN + 1])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn preamble_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(64);
        write_preamble(&mut a, Intent::PairWithSas).await.unwrap();
        assert_eq!(read_preamble(&mut b).await.unwrap(), Intent::PairWithSas);
    }

    #[tokio::test]
    async fn rejects_foreign_preamble() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(b"GET / ").await.unwrap();
        assert!(matches!(
            read_preamble(&mut b).await,
            Err(NetError::Protocol(_))
        ));
    }
}
