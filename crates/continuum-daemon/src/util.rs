use std::net::SocketAddr;

use continuum_core::clock::{Clock, SystemClock, Timestamp};
use tokio::io::{AsyncBufReadExt, BufReader};

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

/// Prints a QR code as dark-on-light, whatever the terminal theme: on a dark
/// theme the plain block characters would render an inverted code, which
/// many scanners reject.
pub fn print_qr(text: &str) {
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .dark_color(qrcode::render::unicode::Dense1x2::Light)
                .light_color(qrcode::render::unicode::Dense1x2::Dark)
                .quiet_zone(true)
                .build();
            // Black text on a white background, so the "light" blocks we
            // draw are white and the gaps are black modules.
            for line in rendered.lines() {
                println!("\x1b[97;40m{line}\x1b[0m");
            }
        }
        Err(err) => eprintln!("could not render QR code: {err}"),
    }
}
