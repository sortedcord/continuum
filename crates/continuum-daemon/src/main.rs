//! `continuum`: Continuum daemon and CLI for Linux.

mod ctx;
mod dbus;
mod pair;
mod quickshare;
mod ring;
mod run;
mod service;
mod util;

use std::net::SocketAddr;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::ctx::{Ctx, short_id};

#[derive(Parser)]
#[command(version, about = "Continuum: seamless Linux + Android continuity")]
struct Cli {
    /// Keyring profile; lets several instances run on one machine.
    #[arg(
        long,
        global = true,
        default_value = "default",
        env = "CONTINUUM_PROFILE"
    )]
    profile: String,
    /// Device name shown to other devices (default: host name).
    #[arg(long, global = true, env = "CONTINUUM_NAME")]
    name: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show this device's identity.
    Id,
    /// List paired devices (with live presence if the daemon is running).
    Peers,
    /// Make a paired device ring, even on silent.
    Page {
        device: String,
        /// How long to ring.
        #[arg(long, default_value_t = 30)]
        seconds: u32,
    },
    /// Show a device's distance live.
    Radar { device: String },
    /// Pair with another device.
    Pair {
        #[command(subcommand)]
        action: PairAction,
    },
    /// Forget a paired device (by id prefix or name).
    Unpair { device: String },
    /// Run the presence daemon.
    Run {
        /// TCP port for sessions (0 = any free port).
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Disable Bluetooth; discover over the LAN only.
        #[arg(long)]
        no_ble: bool,
        /// Seconds between keepalive pings.
        #[arg(long, default_value_t = 10)]
        keepalive: u64,
    },
    /// Quick Share discovery (transfers come in Checkpoint 3).
    Quickshare {
        #[command(subcommand)]
        action: QuickshareAction,
    },
    /// Install or remove the daemon as a systemd user service.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Delete this profile's identity and paired devices.
    Reset {
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PairAction {
    /// Show a QR code and wait to be paired.
    Invite {
        /// Seconds the invite stays valid.
        #[arg(long, default_value_t = 300)]
        ttl: u64,
        /// Refuse pairing by code comparison; QR code only.
        #[arg(long)]
        qr_only: bool,
    },
    /// Join an invite shown by another device.
    Join { invite: String },
    /// Find devices waiting to be paired on the LAN and pair by comparing codes.
    Scan {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    /// Pair by comparing codes with a device at a known address.
    Code { addr: SocketAddr },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Write ~/.config/systemd/user/continuum.service and start it.
    Install,
    /// Stop and remove the user service.
    Uninstall,
}

#[derive(Subcommand)]
enum QuickshareAction {
    /// Wake nearby Android receivers and list Quick Share targets on the LAN.
    Scan {
        #[arg(long, default_value_t = 15)]
        seconds: u64,
    },
    /// Watch for nearby Android devices that are sharing.
    Watch {
        #[arg(long, default_value_t = 120)]
        seconds: u64,
        /// Show this machine as a receiver while a sender is nearby.
        #[arg(long)]
        visible: bool,
    },
}

fn describe(peer: &dbus::PeerInfo) -> String {
    let mut parts = Vec::new();
    if peer.connected {
        parts.push("connected".to_owned());
    }
    if peer.reachable {
        parts.push("on network".to_owned());
    }
    if peer.nearby {
        parts.push(format!("{} ({:.0} dBm)", peer.proximity, peer.rssi));
    }
    if parts.is_empty() {
        "not seen".to_owned()
    } else {
        parts.join(", ")
    }
}

async fn page(profile: &str, device: &str, seconds: u32) -> anyhow::Result<()> {
    use futures::StreamExt as _;
    let daemon = dbus::connect(profile).await?;
    let mut found = daemon.receive_page_found().await?;
    let id = daemon.page(device, seconds).await?;
    println!("Ringing for up to {seconds} s. Press Ctrl-C to stop.");
    let outcome = tokio::select! {
        () = async {
            while let Some(signal) = found.next().await {
                if signal.args().is_ok_and(|a| a.id == id) {
                    return;
                }
            }
        } => "Found it.",
        () = tokio::time::sleep(Duration::from_secs(seconds.into())) => "Stopped ringing.",
        _ = tokio::signal::ctrl_c() => {
            daemon.stop_page(device).await.ok();
            "Cancelled."
        }
    };
    println!("{outcome}");
    Ok(())
}

async fn radar(profile: &str, device: &str) -> anyhow::Result<()> {
    use futures::StreamExt as _;
    use std::io::Write as _;
    let daemon = dbus::connect(profile).await?;
    let peers = daemon.peers().await?;
    let needle = device.to_lowercase();
    let target = peers
        .iter()
        .find(|p| p.id.starts_with(&needle))
        .or_else(|| {
            peers
                .iter()
                .find(|p| p.name.to_lowercase().contains(&needle))
        })
        .ok_or_else(|| anyhow::anyhow!("no paired device matches {device:?}"))?
        .clone();
    println!("Radar for \"{}\" (Ctrl-C to stop)", target.name);
    let draw = |peer: &dbus::PeerInfo| {
        let line = if peer.nearby {
            // Map roughly -100..-30 dBm onto a 40-cell bar.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cells = (((peer.rssi + 100.0) / 70.0).clamp(0.0, 1.0) * 40.0).round() as usize;
            format!(
                "{:<40}  {:>4.0} dBm  {}",
                "█".repeat(cells),
                peer.rssi,
                peer.proximity
            )
        } else {
            format!("{:<40}  out of Bluetooth range", "")
        };
        print!("\r{line:<70}");
        let _ = std::io::stdout().flush();
    };
    draw(&target);
    let mut changes = daemon.receive_peer_changed().await?;
    loop {
        tokio::select! {
            Some(signal) = changes.next() => {
                if let Ok(args) = signal.args() && args.peer.id == target.id {
                    draw(&args.peer);
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    println!();
    Ok(())
}

#[tokio::main]
#[allow(clippy::too_many_lines)] // A flat command dispatch.
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,continuum=info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Id => {
            let ctx = Ctx::load(&cli.profile, cli.name).await?;
            println!("Name:     {}", ctx.device.name);
            println!("Device:   {}", ctx.identity.device_id());
            println!("Profile:  {}", ctx.profile);
            println!("Paired:   {} device(s)", ctx.peers.len());
        }
        Command::Peers => {
            if let Ok(daemon) = dbus::connect(&cli.profile).await {
                let peers = daemon.peers().await?;
                if peers.is_empty() {
                    println!("No paired devices. Run `continuum pair invite` to pair one.");
                }
                for peer in &peers {
                    println!("{}  {:<24} {}", &peer.id[..8], peer.name, describe(peer));
                }
            } else {
                let ctx = Ctx::load(&cli.profile, cli.name).await?;
                if ctx.peers.is_empty() {
                    println!("No paired devices. Run `continuum pair invite` to pair one.");
                }
                for peer in &ctx.peers {
                    println!("{}  {}", short_id(&peer.device_id()), peer.name);
                }
                println!("(daemon not running; start it with `continuum run` for live status)");
            }
        }
        Command::Page { device, seconds } => page(&cli.profile, &device, seconds).await?,
        Command::Radar { device } => radar(&cli.profile, &device).await?,
        Command::Pair { action } => {
            let mut ctx = Ctx::load(&cli.profile, cli.name).await?;
            match action {
                PairAction::Invite { ttl, qr_only } => {
                    pair::invite(&mut ctx, Duration::from_secs(ttl), !qr_only).await?;
                }
                PairAction::Join { invite } => pair::join(&mut ctx, &invite).await?,
                PairAction::Code { addr } => pair::join_sas(&mut ctx, &[addr]).await?,
                PairAction::Scan { seconds } => {
                    let found = pair::scan(Duration::from_secs(seconds)).await?;
                    match found.as_slice() {
                        [] => println!("No device is waiting to be paired."),
                        [addrs] => pair::join_sas(&mut ctx, addrs).await?,
                        many => {
                            println!(
                                "Several devices are waiting; pick one with `continuum pair code <addr>`:"
                            );
                            for addrs in many {
                                println!(
                                    "  {}",
                                    addrs.first().map(ToString::to_string).unwrap_or_default()
                                );
                            }
                        }
                    }
                }
            }
        }
        Command::Unpair { device } => {
            if let Ok(daemon) = dbus::connect(&cli.profile).await {
                let name = daemon.unpair(&device).await?;
                println!(
                    "Unpaired \"{name}\" on both devices and rotated this device's presence key."
                );
            } else {
                let mut ctx = Ctx::load(&cli.profile, cli.name).await?;
                let peer = ctx.find_peer(&device)?.clone();
                ctx.remove_peer(&peer.device_id()).await?;
                println!(
                    "Forgot \"{}\" and rotated this device's presence key.",
                    peer.name
                );
                println!(
                    "The daemon isn't running, so \"{}\" wasn't told; unpair it there too.",
                    peer.name
                );
            }
        }
        Command::Run {
            port,
            no_ble,
            keepalive,
        } => {
            let ctx = Ctx::load(&cli.profile, cli.name).await?;
            run::run(
                ctx,
                run::RunOpts {
                    port,
                    ble: !no_ble,
                    keepalive: Duration::from_secs(keepalive.max(1)),
                },
            )
            .await?;
        }
        Command::Quickshare { action } => match action {
            QuickshareAction::Scan { seconds } => {
                quickshare::scan(Duration::from_secs(seconds)).await?;
            }
            QuickshareAction::Watch { seconds, visible } => {
                let ctx = Ctx::load(&cli.profile, cli.name).await?;
                quickshare::watch(&ctx, Duration::from_secs(seconds), visible).await?;
            }
        },
        Command::Service { action } => match action {
            ServiceAction::Install => service::install().await?,
            ServiceAction::Uninstall => service::uninstall().await?,
        },
        Command::Reset { yes } => {
            anyhow::ensure!(
                yes,
                "this deletes the identity and all pairings; pass --yes to confirm"
            );
            let keyring = continuum_linux::keyring::Keyring::open(&cli.profile).await?;
            keyring.reset().await?;
            println!("Profile \"{}\" reset.", cli.profile);
        }
    }
    Ok(())
}
