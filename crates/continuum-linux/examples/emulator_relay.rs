//! Dev tool for testing against an Android emulator, whose NAT hides mDNS.
//!
//! Publishes a `_continuum._tcp` record *on behalf of* a paired peer, using a
//! rotating id derived from that peer's presence key, and pointing at a local
//! port (set up with `adb forward tcp:<port> tcp:<app port>`). The daemon
//! then resolves it like a real sighting and connects through the forward.
//!
//! Usage: cargo run -p continuum-linux --example emulator_relay -- <profile> <peer-id-prefix> <port>
use continuum_core::entropy::OsEntropy;
use continuum_core::proto::beacon::BeaconFlags;
use continuum_linux::avahi::Avahi;
use continuum_linux::keyring::Keyring;
use continuum_node::records::continuum_record;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, profile, prefix, port] = args.as_slice() else {
        return Err("usage: emulator_relay <profile> <peer-id-prefix> <port>".into());
    };
    let peers = Keyring::open(profile).await?.load_peers().await?;
    let peer = peers
        .iter()
        .find(|p| p.device_id().to_string().starts_with(prefix.as_str()))
        .ok_or("no such paired peer")?;
    let id = peer.presence_key.generate_id(&mut OsEntropy);
    let record = continuum_record(id, port.parse()?, BeaconFlags::LAN);
    let avahi = Avahi::connect().await?;
    let _publication = avahi.publish(&record).await?;
    println!(
        "Publishing {} for \"{}\" on port {port}; Ctrl-C to stop.",
        record.name, peer.name
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
