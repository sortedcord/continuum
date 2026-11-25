//! Publishes a test service and prints what Avahi browsing reports.
//! Usage: cargo run -p continuum-linux --example avahi_probe [service-type]
use std::time::Duration;

use continuum_linux::avahi::{Avahi, ServiceSpec};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "_continuum._tcp.local.".into());
    let avahi = Avahi::connect().await?;
    println!("host: {}", avahi.host_name().await?);
    let publication = avahi
        .publish(&ServiceSpec {
            name: "continuum-probe".into(),
            service_type: kind.clone(),
            port: 47999,
            txt: vec![("v".into(), "1".into())],
        })
        .await?;
    let mut browser = avahi.browse(&kind).await?;
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            Some(event) = browser.next() => println!("{event:?}"),
            () = &mut deadline => break,
        }
    }
    publication.withdraw().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}
