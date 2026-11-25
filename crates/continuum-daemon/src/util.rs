use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use continuum_core::clock::{Clock, SystemClock, Timestamp};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

pub fn now() -> Timestamp {
    SystemClock.now()
}

/// Addresses other devices on the LAN can reach us on, best first.
pub fn local_addrs(port: u16) -> Vec<SocketAddr> {
    let virtual_prefixes = [
        "docker",
        "veth",
        "br-",
        "virbr",
        "tailscale",
        "wg",
        "tun",
        "tap",
    ];
    let mut addrs: Vec<SocketAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|iface| iface.is_oper_up() && !iface.is_loopback() && !iface.is_link_local())
        .filter(|iface| !virtual_prefixes.iter().any(|p| iface.name.starts_with(p)))
        .map(|iface| SocketAddr::new(iface.ip(), port))
        .collect();
    addrs.sort_by_key(|addr| u8::from(!addr.is_ipv4()));
    addrs
}

/// Orders candidate peer addresses: private IPv4 first, then IPv6, and
/// container bridges last.
pub fn rank_addrs(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut addrs: Vec<_> = addrs
        .into_iter()
        .filter(|a| !a.ip().is_loopback())
        .collect();
    addrs.sort_by_key(|addr| match addr.ip() {
        IpAddr::V4(v4) if v4.octets()[0] == 172 && v4.octets()[1] == 17 => 3,
        IpAddr::V4(_) => 0,
        IpAddr::V6(v6) if v6.is_unicast_link_local() => 2,
        IpAddr::V6(_) => 1,
    });
    addrs
}

/// Connects to the first address that answers.
pub async fn connect_any(addrs: &[SocketAddr]) -> anyhow::Result<TcpStream> {
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
    anyhow::bail!("none of {} address(es) answered", addrs.len())
}

/// Asks a yes/no question on the terminal.
pub async fn confirm(question: &str) -> bool {
    println!("{question} [y/N]");
    let mut line = String::new();
    let mut stdin = BufReader::new(tokio::io::stdin());
    if stdin.read_line(&mut line).await.is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes")
}

pub fn print_qr(text: &str) {
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!("{rendered}");
        }
        Err(err) => eprintln!("could not render QR code: {err}"),
    }
}
