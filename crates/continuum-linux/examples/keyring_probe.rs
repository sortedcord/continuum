//! Creates (or loads) an identity in a throwaway keyring profile, then
//! deletes it again.
use continuum_core::entropy::OsEntropy;
use continuum_linux::keyring::Keyring;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let keyring = Keyring::open("probe").await?;
    let first = keyring.load_or_create_identity(&mut OsEntropy).await?;
    let again = keyring.load_or_create_identity(&mut OsEntropy).await?;
    println!(
        "identity {} stable: {}",
        first.device_id(),
        first.device_id() == again.device_id()
    );
    keyring.save_peers(&[]).await?;
    println!("peers: {}", keyring.load_peers().await?.len());
    keyring.reset().await?;
    Ok(())
}
