//! `continuum`: Continuum daemon and CLI for Linux.

mod ctx;
mod pair;
mod quickshare;
mod run;
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
    /// List paired devices.
    Peers,
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

#[tokio::main]
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
            let ctx = Ctx::load(&cli.profile, cli.name).await?;
            if ctx.peers.is_empty() {
                println!("No paired devices. Run `continuum pair invite` to pair one.");
            }
            for peer in &ctx.peers {
                println!("{}  {}", short_id(&peer.device_id()), peer.name);
            }
        }
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
            let mut ctx = Ctx::load(&cli.profile, cli.name).await?;
            let peer = ctx.find_peer(&device)?.clone();
            ctx.remove_peer(&peer.device_id()).await?;
            println!("Forgot \"{}\".", peer.name);
            println!(
                "Note: it can still recognise this device's beacons until the presence key is rotated (not implemented yet)."
            );
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
