//! Presence registry: turns raw discovery sightings into per-peer state.
//!
//! Adapters report every BLE beacon and mDNS record they see as an
//! [`Observation`]. The registry
//!
//! 1. resolves the rotating identifier against the presence keys of trusted
//!    peers (anything unresolvable is ignored, unless it is in pairing mode),
//! 2. merges BLE and LAN sightings of the same peer,
//! 3. smooths RSSI into a [`ProximityBand`] with hysteresis, and
//! 4. expires peers that stop being seen.
//!
//! It emits [`PresenceEvent`]s describing each change. A sighting is only a
//! hint: being "present" does not mean authenticated. The session layer
//! still has to complete a handshake before anything is trusted.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::net::SocketAddr;
use std::time::Duration;

use continuum_proto::ResolvableId;

use crate::clock::Timestamp;
use crate::identity::DeviceId;
use crate::rid::PresenceKey;
use crate::trust::TrustedPeer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Transport {
    Ble,
    Lan,
}

/// One sighting of an advertisement.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub transport: Transport,
    pub id: ResolvableId,
    /// The advertiser is in pairing mode and its identifier is random.
    pub pairing: bool,
    /// Received signal strength in dBm (BLE only).
    pub rssi: Option<i16>,
    /// Where the advertised service can be reached (LAN only).
    pub addr: Option<SocketAddr>,
    pub at: Timestamp,
}

impl Observation {
    pub fn ble(id: ResolvableId, rssi: i16, at: Timestamp) -> Self {
        Self {
            transport: Transport::Ble,
            id,
            pairing: false,
            rssi: Some(rssi),
            addr: None,
            at,
        }
    }

    pub fn lan(id: ResolvableId, addr: SocketAddr, at: Timestamp) -> Self {
        Self {
            transport: Transport::Lan,
            id,
            pairing: false,
            rssi: None,
            addr: Some(addr),
            at,
        }
    }

    #[must_use]
    pub fn in_pairing_mode(mut self) -> Self {
        self.pairing = true;
        self
    }
}

/// Coarse distance, ordered from closest to farthest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProximityBand {
    /// Roughly arm's length.
    Immediate,
    /// Same room.
    Near,
    /// In radio range, but not close.
    Far,
}

/// RSSI thresholds for one band, in dBm. `exit` is lower than `enter` so a
/// signal hovering at the boundary does not flap between bands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threshold {
    pub enter: f32,
    pub exit: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegistryConfig {
    /// BLE presence is dropped after this long without a beacon.
    pub ble_ttl: Duration,
    /// LAN presence is dropped after this long without a record refresh.
    pub lan_ttl: Duration,
    /// Pairing candidates are dropped after this long unseen.
    pub candidate_ttl: Duration,
    /// Weight of each new RSSI sample in the moving average, in `(0, 1]`.
    pub rssi_smoothing: f32,
    pub immediate: Threshold,
    pub near: Threshold,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            ble_ttl: Duration::from_secs(30),
            lan_ttl: Duration::from_secs(150),
            candidate_ttl: Duration::from_secs(30),
            rssi_smoothing: 0.3,
            immediate: Threshold {
                enter: -55.0,
                exit: -62.0,
            },
            near: Threshold {
                enter: -75.0,
                exit: -82.0,
            },
        }
    }
}

impl RegistryConfig {
    fn band(&self, current: Option<ProximityBand>, rssi: f32) -> ProximityBand {
        let classify = |immediate: f32, near: f32| {
            if rssi >= immediate {
                ProximityBand::Immediate
            } else if rssi >= near {
                ProximityBand::Near
            } else {
                ProximityBand::Far
            }
        };
        let entered = classify(self.immediate.enter, self.near.enter);
        let Some(current) = current else {
            return entered;
        };
        if entered <= current {
            // Moving closer (or staying) requires crossing an enter threshold.
            entered
        } else {
            // Moving away requires dropping below the current band's exit
            // threshold.
            classify(self.immediate.exit, self.near.exit).max(current)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceEvent {
    Appeared {
        peer: DeviceId,
        transport: Transport,
    },
    Lost {
        peer: DeviceId,
        transport: Transport,
    },
    ProximityChanged {
        peer: DeviceId,
        band: ProximityBand,
    },
    /// The set of LAN addresses for the peer changed; see
    /// [`PresenceRegistry::presence`].
    AddressesChanged {
        peer: DeviceId,
    },
    PairingCandidateAppeared {
        id: ResolvableId,
        transport: Transport,
    },
    PairingCandidateLost {
        id: ResolvableId,
        transport: Transport,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlePresence {
    pub last_seen: Timestamp,
    /// Smoothed RSSI in dBm.
    pub rssi: Option<f32>,
    pub band: Option<ProximityBand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LanPresence {
    pub last_seen: Timestamp,
    /// Addresses per advertised identifier. Several identifiers can be live
    /// at once while an advertisement rotates.
    adverts: BTreeMap<ResolvableId, BTreeSet<SocketAddr>>,
}

impl LanPresence {
    pub fn addrs(&self) -> BTreeSet<SocketAddr> {
        self.adverts.values().flatten().copied().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PeerPresence {
    pub ble: Option<BlePresence>,
    pub lan: Option<LanPresence>,
}

impl PeerPresence {
    pub fn is_nearby(&self) -> bool {
        self.ble.is_some()
    }

    pub fn is_reachable(&self) -> bool {
        self.lan.is_some()
    }

    fn is_empty(&self) -> bool {
        self.ble.is_none() && self.lan.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairingCandidate {
    pub id: ResolvableId,
    pub transport: Transport,
    pub last_seen: Timestamp,
    pub rssi: Option<i16>,
    pub addr: Option<SocketAddr>,
}

/// Bound on the resolution caches. Hitting it simply clears the cache.
const CACHE_LIMIT: usize = 4096;

#[derive(Debug, Default)]
pub struct PresenceRegistry {
    config: RegistryConfig,
    directory: Vec<(DeviceId, PresenceKey)>,
    resolved: HashMap<ResolvableId, DeviceId>,
    unresolved: HashSet<ResolvableId>,
    peers: BTreeMap<DeviceId, PeerPresence>,
    candidates: BTreeMap<(Transport, ResolvableId), PairingCandidate>,
}

impl PresenceRegistry {
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Replaces the set of peers whose identifiers can be resolved. Peers
    /// that are no longer trusted are dropped and reported as lost.
    pub fn set_trusted(
        &mut self,
        peers: impl IntoIterator<Item = (DeviceId, PresenceKey)>,
    ) -> Vec<PresenceEvent> {
        self.directory = peers.into_iter().collect();
        self.resolved.clear();
        self.unresolved.clear();

        let trusted: HashSet<DeviceId> = self.directory.iter().map(|(id, _)| *id).collect();
        let removed: Vec<DeviceId> = self
            .peers
            .keys()
            .filter(|id| !trusted.contains(id))
            .copied()
            .collect();
        let mut events = Vec::new();
        for peer in removed {
            if let Some(presence) = self.peers.remove(&peer) {
                lost_events(peer, &presence, &mut events);
            }
        }
        events
    }

    pub fn set_trusted_peers(&mut self, peers: &[TrustedPeer]) -> Vec<PresenceEvent> {
        self.set_trusted(
            peers
                .iter()
                .map(|peer| (peer.device_id(), peer.presence_key.clone())),
        )
    }

    pub fn presence(&self, peer: &DeviceId) -> Option<&PeerPresence> {
        self.peers.get(peer)
    }

    pub fn peers(&self) -> impl Iterator<Item = (&DeviceId, &PeerPresence)> {
        self.peers.iter()
    }

    pub fn pairing_candidates(&self) -> impl Iterator<Item = &PairingCandidate> {
        self.candidates.values()
    }

    fn resolve(&mut self, id: &ResolvableId) -> Option<DeviceId> {
        if let Some(peer) = self.resolved.get(id) {
            return Some(*peer);
        }
        if self.unresolved.contains(id) {
            return None;
        }
        let found = self
            .directory
            .iter()
            .find(|(_, key)| key.resolves(id))
            .map(|(peer, _)| *peer);
        if let Some(peer) = found {
            if self.resolved.len() >= CACHE_LIMIT {
                self.resolved.clear();
            }
            self.resolved.insert(*id, peer);
        } else {
            if self.unresolved.len() >= CACHE_LIMIT {
                self.unresolved.clear();
            }
            self.unresolved.insert(*id);
        }
        found
    }

    pub fn ingest(&mut self, obs: &Observation) -> Vec<PresenceEvent> {
        let mut events = Vec::new();
        if obs.pairing {
            self.ingest_candidate(obs, &mut events);
            return events;
        }
        let Some(peer) = self.resolve(&obs.id) else {
            return events;
        };
        let config = &self.config;
        let presence = self.peers.entry(peer).or_default();
        match obs.transport {
            Transport::Ble => ingest_ble(config, peer, presence, obs, &mut events),
            Transport::Lan => ingest_lan(peer, presence, obs, &mut events),
        }
        events
    }

    fn ingest_candidate(&mut self, obs: &Observation, events: &mut Vec<PresenceEvent>) {
        let key = (obs.transport, obs.id);
        let candidate = self.candidates.entry(key).or_insert_with(|| {
            events.push(PresenceEvent::PairingCandidateAppeared {
                id: obs.id,
                transport: obs.transport,
            });
            PairingCandidate {
                id: obs.id,
                transport: obs.transport,
                last_seen: obs.at,
                rssi: None,
                addr: None,
            }
        });
        candidate.last_seen = candidate.last_seen.max(obs.at);
        candidate.rssi = obs.rssi.or(candidate.rssi);
        candidate.addr = obs.addr.or(candidate.addr);
    }

    /// Records that an advertisement went away (e.g. an mDNS goodbye).
    pub fn withdraw(&mut self, transport: Transport, id: &ResolvableId) -> Vec<PresenceEvent> {
        let mut events = Vec::new();
        if self.candidates.remove(&(transport, *id)).is_some() {
            events.push(PresenceEvent::PairingCandidateLost { id: *id, transport });
            return events;
        }
        let Some(peer) = self.resolve(id) else {
            return events;
        };
        let Some(presence) = self.peers.get_mut(&peer) else {
            return events;
        };
        match transport {
            Transport::Lan => {
                let Some(lan) = presence.lan.as_mut() else {
                    return events;
                };
                if lan.adverts.remove(id).is_none() {
                    return events;
                }
                if lan.adverts.is_empty() {
                    presence.lan = None;
                    events.push(PresenceEvent::Lost { peer, transport });
                } else {
                    events.push(PresenceEvent::AddressesChanged { peer });
                }
            }
            Transport::Ble => {
                if presence.ble.take().is_some() {
                    events.push(PresenceEvent::Lost { peer, transport });
                }
            }
        }
        if presence.is_empty() {
            self.peers.remove(&peer);
        }
        events
    }

    /// Drops everything not seen within its time-to-live.
    pub fn expire(&mut self, now: Timestamp) -> Vec<PresenceEvent> {
        let mut events = Vec::new();
        let config = &self.config;
        let stale = |last_seen: Timestamp, ttl: Duration| now.saturating_since(last_seen) >= ttl;

        self.peers.retain(|peer, presence| {
            if presence
                .ble
                .as_ref()
                .is_some_and(|ble| stale(ble.last_seen, config.ble_ttl))
            {
                presence.ble = None;
                events.push(PresenceEvent::Lost {
                    peer: *peer,
                    transport: Transport::Ble,
                });
            }
            if presence
                .lan
                .as_ref()
                .is_some_and(|lan| stale(lan.last_seen, config.lan_ttl))
            {
                presence.lan = None;
                events.push(PresenceEvent::Lost {
                    peer: *peer,
                    transport: Transport::Lan,
                });
            }
            !presence.is_empty()
        });

        self.candidates.retain(|(transport, id), candidate| {
            let keep = !stale(candidate.last_seen, config.candidate_ttl);
            if !keep {
                events.push(PresenceEvent::PairingCandidateLost {
                    id: *id,
                    transport: *transport,
                });
            }
            keep
        });
        events
    }
}

fn ingest_ble(
    config: &RegistryConfig,
    peer: DeviceId,
    presence: &mut PeerPresence,
    obs: &Observation,
    events: &mut Vec<PresenceEvent>,
) {
    let ble = presence.ble.get_or_insert_with(|| {
        events.push(PresenceEvent::Appeared {
            peer,
            transport: Transport::Ble,
        });
        BlePresence {
            last_seen: obs.at,
            rssi: None,
            band: None,
        }
    });
    ble.last_seen = ble.last_seen.max(obs.at);

    let Some(sample) = obs.rssi.map(f32::from) else {
        return;
    };
    let smoothed = match ble.rssi {
        Some(previous) => previous + config.rssi_smoothing * (sample - previous),
        None => sample,
    };
    ble.rssi = Some(smoothed);
    let band = config.band(ble.band, smoothed);
    if ble.band != Some(band) {
        ble.band = Some(band);
        events.push(PresenceEvent::ProximityChanged { peer, band });
    }
}

fn ingest_lan(
    peer: DeviceId,
    presence: &mut PeerPresence,
    obs: &Observation,
    events: &mut Vec<PresenceEvent>,
) {
    let lan = presence.lan.get_or_insert_with(|| {
        events.push(PresenceEvent::Appeared {
            peer,
            transport: Transport::Lan,
        });
        LanPresence {
            last_seen: obs.at,
            adverts: BTreeMap::new(),
        }
    });
    lan.last_seen = lan.last_seen.max(obs.at);
    let addrs = lan.adverts.entry(obs.id).or_default();
    if let Some(addr) = obs.addr
        && addrs.insert(addr)
    {
        events.push(PresenceEvent::AddressesChanged { peer });
    }
}

fn lost_events(peer: DeviceId, presence: &PeerPresence, events: &mut Vec<PresenceEvent>) {
    if presence.ble.is_some() {
        events.push(PresenceEvent::Lost {
            peer,
            transport: Transport::Ble,
        });
    }
    if presence.lan.is_some() {
        events.push(PresenceEvent::Lost {
            peer,
            transport: Transport::Lan,
        });
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty)]
mod tests {
    use super::*;
    use crate::entropy::DeterministicEntropy;
    use crate::identity::DeviceIdentity;
    use crate::rid::random_id;

    struct Fixture {
        registry: PresenceRegistry,
        phone: DeviceIdentity,
        entropy: DeterministicEntropy,
    }

    impl Fixture {
        fn new() -> Self {
            let mut entropy = DeterministicEntropy::new(99);
            let phone = DeviceIdentity::generate(&mut entropy);
            let mut registry = PresenceRegistry::new(RegistryConfig::default());
            registry.set_trusted([(phone.device_id(), phone.presence_key().clone())]);
            Self {
                registry,
                phone,
                entropy,
            }
        }

        fn phone_id(&mut self) -> ResolvableId {
            self.phone.presence_key().generate_id(&mut self.entropy)
        }
    }

    fn at(secs: u64) -> Timestamp {
        Timestamp::from_millis(secs * 1000)
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([192, 168, 1, 20], port))
    }

    #[test]
    fn trusted_ble_beacon_appears_with_band() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let id = fx.phone_id();
        let events = fx.registry.ingest(&Observation::ble(id, -50, at(0)));
        assert_eq!(
            events,
            vec![
                PresenceEvent::Appeared {
                    peer,
                    transport: Transport::Ble
                },
                PresenceEvent::ProximityChanged {
                    peer,
                    band: ProximityBand::Immediate
                },
            ]
        );
        assert!(fx.registry.presence(&peer).unwrap().is_nearby());
    }

    #[test]
    fn untrusted_beacons_are_ignored() {
        let mut fx = Fixture::new();
        let stranger = random_id(&mut fx.entropy);
        assert!(
            fx.registry
                .ingest(&Observation::ble(stranger, -40, at(0)))
                .is_empty()
        );
        assert_eq!(fx.registry.peers().count(), 0);
    }

    #[test]
    fn rotated_ids_resolve_to_same_peer() {
        let mut fx = Fixture::new();
        let (first, second) = (fx.phone_id(), fx.phone_id());
        assert_ne!(first, second);
        fx.registry.ingest(&Observation::ble(first, -70, at(0)));
        let events = fx.registry.ingest(&Observation::ble(second, -70, at(1)));
        assert!(
            events.is_empty(),
            "no new appearance after rotation: {events:?}"
        );
        assert_eq!(fx.registry.peers().count(), 1);
    }

    #[test]
    fn ble_and_lan_merge_into_one_peer() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let (ble_id, lan_id) = (fx.phone_id(), fx.phone_id());
        fx.registry.ingest(&Observation::ble(ble_id, -70, at(0)));
        let events = fx
            .registry
            .ingest(&Observation::lan(lan_id, addr(4000), at(0)));
        assert_eq!(
            events,
            vec![
                PresenceEvent::Appeared {
                    peer,
                    transport: Transport::Lan
                },
                PresenceEvent::AddressesChanged { peer },
            ]
        );
        let presence = fx.registry.presence(&peer).unwrap();
        assert!(presence.is_nearby() && presence.is_reachable());
        assert_eq!(
            presence.lan.as_ref().unwrap().addrs(),
            BTreeSet::from([addr(4000)])
        );
    }

    #[test]
    fn ble_expires_after_ttl() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let id = fx.phone_id();
        fx.registry.ingest(&Observation::ble(id, -70, at(0)));
        assert!(fx.registry.expire(at(29)).is_empty());
        assert_eq!(
            fx.registry.expire(at(30)),
            vec![PresenceEvent::Lost {
                peer,
                transport: Transport::Ble
            }]
        );
        assert!(fx.registry.presence(&peer).is_none());
    }

    #[test]
    fn lan_withdraw_waits_for_last_advert() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let (old, new) = (fx.phone_id(), fx.phone_id());
        fx.registry
            .ingest(&Observation::lan(old, addr(4000), at(0)));
        fx.registry
            .ingest(&Observation::lan(new, addr(4001), at(1)));

        assert_eq!(
            fx.registry.withdraw(Transport::Lan, &old),
            vec![PresenceEvent::AddressesChanged { peer }]
        );
        assert!(fx.registry.presence(&peer).unwrap().is_reachable());
        assert_eq!(
            fx.registry.withdraw(Transport::Lan, &new),
            vec![PresenceEvent::Lost {
                peer,
                transport: Transport::Lan
            }]
        );
        assert!(fx.registry.presence(&peer).is_none());
    }

    #[test]
    fn proximity_band_has_hysteresis() {
        use ProximityBand::{Far, Immediate, Near};
        let config = RegistryConfig::default();
        assert_eq!(config.band(None, -50.0), Immediate);
        // Just below the enter threshold but above exit: stay.
        assert_eq!(config.band(Some(Immediate), -58.0), Immediate);
        assert_eq!(config.band(Some(Immediate), -65.0), Near);
        // Approaching needs the enter threshold.
        assert_eq!(config.band(Some(Near), -58.0), Near);
        assert_eq!(config.band(Some(Near), -54.0), Immediate);
        assert_eq!(config.band(Some(Near), -80.0), Near);
        assert_eq!(config.band(Some(Near), -83.0), Far);
        assert_eq!(config.band(Some(Immediate), -95.0), Far);
    }

    #[test]
    fn rssi_is_smoothed() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let id = fx.phone_id();
        fx.registry.ingest(&Observation::ble(id, -50, at(0)));
        // One deep fade should not knock the peer out of Immediate.
        let events = fx.registry.ingest(&Observation::ble(id, -70, at(1)));
        assert!(events.is_empty(), "{events:?}");
        let rssi = fx
            .registry
            .presence(&peer)
            .unwrap()
            .ble
            .as_ref()
            .unwrap()
            .rssi;
        assert_eq!(rssi, Some(-56.0));
    }

    #[test]
    fn pairing_beacons_become_candidates() {
        let mut fx = Fixture::new();
        let id = random_id(&mut fx.entropy);
        let obs = Observation::ble(id, -45, at(0)).in_pairing_mode();
        assert_eq!(
            fx.registry.ingest(&obs),
            vec![PresenceEvent::PairingCandidateAppeared {
                id,
                transport: Transport::Ble
            }]
        );
        assert!(fx.registry.ingest(&obs).is_empty());
        assert_eq!(fx.registry.pairing_candidates().count(), 1);
        assert_eq!(
            fx.registry.expire(at(30)),
            vec![PresenceEvent::PairingCandidateLost {
                id,
                transport: Transport::Ble
            }]
        );
    }

    #[test]
    fn untrusting_a_peer_reports_it_lost() {
        let mut fx = Fixture::new();
        let peer = fx.phone.device_id();
        let id = fx.phone_id();
        fx.registry.ingest(&Observation::ble(id, -70, at(0)));
        assert_eq!(
            fx.registry.set_trusted([]),
            vec![PresenceEvent::Lost {
                peer,
                transport: Transport::Ble
            }]
        );
        // And its identifiers no longer resolve.
        assert!(
            fx.registry
                .ingest(&Observation::ble(id, -70, at(1)))
                .is_empty()
        );
    }
}
