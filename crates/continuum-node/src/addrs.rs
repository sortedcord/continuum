use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::net::TcpStream;

use crate::NodeError;

/// Orders candidate addresses: private IPv4 first, then IPv6, then container
/// bridges and loopback.
pub fn rank_addrs(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut addrs: Vec<_> = addrs.into_iter().collect();
    addrs.sort_by_key(|addr| match addr.ip() {
        ip if ip.is_loopback() => 4,
        IpAddr::V4(v4) if v4.octets()[0] == 172 && v4.octets()[1] == 17 => 3,
        IpAddr::V4(_) => 0,
        IpAddr::V6(v6) if v6.is_unicast_link_local() => 2,
        IpAddr::V6(_) => 1,
    });
    addrs.dedup();
    addrs
}

/// Connects to the first address that answers within three seconds.
///
/// # Errors
/// Fails if no address answers.
pub async fn connect_any(addrs: &[SocketAddr]) -> Result<TcpStream, NodeError> {
    for addr in addrs {
        match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => {
                stream.set_nodelay(true).ok();
                return Ok(stream);
            }
            Ok(Err(err)) => tracing::debug!(%addr, %err, "connect failed"),
            Err(_) => tracing::debug!(%addr, "connect timed out"),
        }
    }
    Err(NodeError::Unreachable(addrs.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_lan_ipv4_first() {
        let addrs: Vec<SocketAddr> = [
            "127.0.0.1:1",
            "[fe80::1%2]:1",
            "172.17.0.1:1",
            "192.168.0.5:1",
            "[2001:db8::1]:1",
        ]
        .iter()
        .map(|a| a.parse().unwrap())
        .collect();
        let ranked: Vec<String> = rank_addrs(addrs).iter().map(ToString::to_string).collect();
        assert_eq!(
            ranked,
            [
                "192.168.0.5:1",
                "[2001:db8::1]:1",
                "[fe80::1%2]:1",
                "172.17.0.1:1",
                "127.0.0.1:1"
            ]
        );
    }
}
