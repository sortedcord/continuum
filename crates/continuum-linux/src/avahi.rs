//! Avahi over D-Bus: publish and browse DNS-SD services.
//!
//! Uses the system Avahi daemon rather than a second mDNS responder, so we
//! don't fight it for port 5353.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, SocketAddrV6};

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use zbus::{Connection, OwnedObjectPath, proxy};

const IF_UNSPEC: i32 = -1;
const PROTO_UNSPEC: i32 = -1;

#[proxy(
    interface = "org.freedesktop.Avahi.Server2",
    default_service = "org.freedesktop.Avahi",
    default_path = "/"
)]
trait Server2 {
    fn get_host_name(&self) -> zbus::Result<String>;

    fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;

    fn service_browser_prepare(
        &self,
        interface: i32,
        protocol: i32,
        r#type: &str,
        domain: &str,
        flags: u32,
    ) -> zbus::Result<OwnedObjectPath>;

    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn resolve_service(
        &self,
        interface: i32,
        protocol: i32,
        name: &str,
        r#type: &str,
        domain: &str,
        aprotocol: i32,
        flags: u32,
    ) -> zbus::Result<(
        i32,
        i32,
        String,
        String,
        String,
        String,
        i32,
        String,
        u16,
        Vec<Vec<u8>>,
        u32,
    )>;
}

#[proxy(
    interface = "org.freedesktop.Avahi.EntryGroup",
    default_service = "org.freedesktop.Avahi"
)]
trait EntryGroup {
    fn free(&self) -> zbus::Result<()>;
    fn commit(&self) -> zbus::Result<()>;
    fn reset(&self) -> zbus::Result<()>;

    #[allow(clippy::too_many_arguments)]
    fn add_service(
        &self,
        interface: i32,
        protocol: i32,
        flags: u32,
        name: &str,
        r#type: &str,
        domain: &str,
        host: &str,
        port: u16,
        txt: Vec<Vec<u8>>,
    ) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.Avahi.ServiceBrowser",
    default_service = "org.freedesktop.Avahi"
)]
trait ServiceBrowser {
    fn start(&self) -> zbus::Result<()>;
    fn free(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn item_new(
        &self,
        interface: i32,
        protocol: i32,
        name: String,
        r#type: String,
        domain: String,
        flags: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn item_remove(
        &self,
        interface: i32,
        protocol: i32,
        name: String,
        r#type: String,
        domain: String,
        flags: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn failure(&self, error: String) -> zbus::Result<()>;
}

#[derive(Debug, thiserror::Error)]
pub enum AvahiError {
    #[error("D-Bus: {0}")]
    DBus(#[from] zbus::Error),
    #[error("Avahi returned an invalid address: {0}")]
    BadAddress(String),
}

/// Avahi wants `_type._tcp`, without the `.local.` suffix.
fn avahi_type(service_type: &str) -> &str {
    service_type
        .trim_end_matches('.')
        .trim_end_matches(".local")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    pub name: String,
    /// e.g. `_continuum._tcp.local.`
    pub service_type: String,
    pub port: u16,
    pub txt: Vec<(String, String)>,
}

impl ServiceSpec {
    fn txt_records(&self) -> Vec<Vec<u8>> {
        self.txt
            .iter()
            .map(|(key, value)| format!("{key}={value}").into_bytes())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedService {
    pub name: String,
    pub host: String,
    pub addr: SocketAddr,
    pub txt: Vec<(String, String)>,
}

impl ResolvedService {
    pub fn txt(&self, key: &str) -> Option<&str> {
        self.txt
            .iter()
            .find_map(|(k, v)| (k == key).then_some(v.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowseEvent {
    /// A service resolved, on one interface and address family. The same
    /// service can be reported several times with different addresses.
    Resolved(ResolvedService),
    /// The service is gone from every interface.
    Removed { name: String },
}

#[derive(Debug, Clone)]
pub struct Avahi {
    conn: Connection,
    server: Server2Proxy<'static>,
}

impl Avahi {
    /// # Errors
    /// Fails if the system bus or the Avahi daemon is unavailable.
    pub async fn connect() -> Result<Self, AvahiError> {
        let conn = Connection::system().await?;
        let server = Server2Proxy::new(&conn).await?;
        Ok(Self { conn, server })
    }

    /// # Errors
    /// Fails if Avahi is unreachable.
    pub async fn host_name(&self) -> Result<String, AvahiError> {
        Ok(self.server.get_host_name().await?)
    }

    /// Publishes a service on all interfaces until withdrawn.
    ///
    /// # Errors
    /// Fails if Avahi rejects the service.
    pub async fn publish(&self, spec: &ServiceSpec) -> Result<Publication, AvahiError> {
        let path = self.server.entry_group_new().await?;
        let group = EntryGroupProxy::builder(&self.conn)
            .path(path)
            .build()
            .await?;
        let publication = Publication { group };
        publication.add(spec).await?;
        Ok(publication)
    }

    /// Browses for a service type. Dropping the returned [`Browser`] stops
    /// browsing.
    ///
    /// # Errors
    /// Fails if Avahi is unreachable.
    pub async fn browse(&self, service_type: &str) -> Result<Browser, AvahiError> {
        let service_type = avahi_type(service_type).to_owned();
        let path = self
            .server
            .service_browser_prepare(IF_UNSPEC, PROTO_UNSPEC, &service_type, "", 0)
            .await?;
        let browser = ServiceBrowserProxy::builder(&self.conn)
            .path(path)
            .build()
            .await?;
        // Subscribe before starting so no early signal is lost.
        let mut added = browser.receive_item_new().await?;
        let mut removed = browser.receive_item_remove().await?;
        let mut failed = browser.receive_failure().await?;
        browser.start().await?;

        let (tx, rx) = mpsc::channel(64);
        let server = self.server.clone();
        let task = tokio::spawn(async move {
            // Which (interface, protocol) pairs currently carry each name.
            let mut live: HashMap<String, HashSet<(i32, i32)>> = HashMap::new();
            loop {
                tokio::select! {
                    Some(signal) = added.next() => {
                        let Ok(args) = signal.args() else { continue };
                        live.entry(args.name.clone())
                            .or_default()
                            .insert((args.interface, args.protocol));
                        let (server, tx) = (server.clone(), tx.clone());
                        let (iface, proto, name, kind, domain) = (
                            args.interface,
                            args.protocol,
                            args.name.clone(),
                            args.r#type.clone(),
                            args.domain.clone(),
                        );
                        tokio::spawn(async move {
                            match resolve(&server, iface, proto, &name, &kind, &domain).await {
                                Ok(service) => {
                                    let _ = tx.send(BrowseEvent::Resolved(service)).await;
                                }
                                Err(err) => tracing::debug!(%name, %err, "resolve failed"),
                            }
                        });
                    }
                    Some(signal) = removed.next() => {
                        let Ok(args) = signal.args() else { continue };
                        let gone = live.get_mut(&args.name).is_some_and(|set| {
                            set.remove(&(args.interface, args.protocol));
                            set.is_empty()
                        });
                        if gone {
                            live.remove(&args.name);
                            let event = BrowseEvent::Removed { name: args.name.clone() };
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(signal) = failed.next() => {
                        if let Ok(args) = signal.args() {
                            tracing::warn!(error = %args.error, "Avahi browser failed");
                        }
                        break;
                    }
                    () = tx.closed() => break,
                    else => break,
                }
            }
            let _ = browser.free().await;
        });
        Ok(Browser { rx, task })
    }
}

async fn resolve(
    server: &Server2Proxy<'static>,
    interface: i32,
    protocol: i32,
    name: &str,
    kind: &str,
    domain: &str,
) -> Result<ResolvedService, AvahiError> {
    let (iface, _, name, _, _, host, _, address, port, txt, _) = server
        .resolve_service(interface, protocol, name, kind, domain, PROTO_UNSPEC, 0)
        .await?;
    let ip: IpAddr = address
        .parse()
        .map_err(|_| AvahiError::BadAddress(address.clone()))?;
    let addr = match ip {
        IpAddr::V6(v6) if v6.is_unicast_link_local() => SocketAddr::V6(SocketAddrV6::new(
            v6,
            port,
            0,
            u32::try_from(iface).unwrap_or(0),
        )),
        _ => SocketAddr::new(ip, port),
    };
    Ok(ResolvedService {
        name,
        host,
        addr,
        txt: txt.iter().filter_map(|record| parse_txt(record)).collect(),
    })
}

fn parse_txt(record: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(record).ok()?;
    let (key, value) = text.split_once('=').unwrap_or((text, ""));
    Some((key.to_owned(), value.to_owned()))
}

/// A published service. Call [`Publication::withdraw`] to remove it;
/// dropping it also removes it, in the background.
#[derive(Debug)]
pub struct Publication {
    group: EntryGroupProxy<'static>,
}

impl Publication {
    async fn add(&self, spec: &ServiceSpec) -> Result<(), AvahiError> {
        self.group
            .add_service(
                IF_UNSPEC,
                PROTO_UNSPEC,
                0,
                &spec.name,
                avahi_type(&spec.service_type),
                "",
                "",
                spec.port,
                spec.txt_records(),
            )
            .await?;
        self.group.commit().await?;
        Ok(())
    }

    /// Replaces the published service, e.g. after rotating its name.
    ///
    /// # Errors
    /// Fails if Avahi rejects the service.
    pub async fn update(&self, spec: &ServiceSpec) -> Result<(), AvahiError> {
        self.group.reset().await?;
        self.add(spec).await
    }

    /// # Errors
    /// Fails if Avahi is unreachable.
    pub async fn withdraw(self) -> Result<(), AvahiError> {
        self.group.free().await?;
        Ok(())
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        let group = self.group.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = group.free().await;
            });
        }
    }
}

#[derive(Debug)]
pub struct Browser {
    rx: mpsc::Receiver<BrowseEvent>,
    task: JoinHandle<()>,
}

impl Browser {
    pub async fn next(&mut self) -> Option<BrowseEvent> {
        self.rx.recv().await
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_local_suffix() {
        assert_eq!(avahi_type("_continuum._tcp.local."), "_continuum._tcp");
        assert_eq!(avahi_type("_FC9F5ED42C8A._tcp"), "_FC9F5ED42C8A._tcp");
    }

    #[test]
    fn parses_txt_records() {
        assert_eq!(parse_txt(b"v=1"), Some(("v".into(), "1".into())));
        assert_eq!(parse_txt(b"flag"), Some(("flag".into(), String::new())));
        assert_eq!(parse_txt(b"n=a=b"), Some(("n".into(), "a=b".into())));
    }
}
