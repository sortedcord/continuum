---
title: Discovery Protocol (Spec)
description: How Continuum devices find each other without leaking who they are.
---

Status: **draft, Checkpoint 1.** Reference implementation in `crates/continuum-core` and `crates/continuum-proto`.

## Principles

1. **Discovery yields hints, never trust.** A sighting only says "an advertisement that resolves to peer X was seen". Identity is proven by the session handshake.
2. **Nothing stable is broadcast.** No device names, hardware IDs or long-term keys appear in BLE adverts or mDNS records.
3. **The core does no I/O.** Platform adapters feed observations into a deterministic registry, so all of it is testable without radios.

## Tiers

| Tier | Purpose | Carries |
| --- | --- | --- |
| BLE beacon | Physical proximity, RSSI | Rotating resolvable ID, flags |
| mDNS / DNS-SD `_continuum._tcp` | LAN reachability | Rotating resolvable ID as instance name, port |
| Authenticated session | Identity, capabilities | Everything else |

## Identity

Each device generates, once:

- an **Ed25519** signing key (the identity),
- an **X25519** static key (session key agreement),
- a 32-byte **presence key** (rotating identifiers).

`DeviceId = SHA-256("continuum/device-id/v1" || ed25519_pk)[..16]`. It is never broadcast.

## Resolvable identifiers

Modelled on Bluetooth resolvable private addresses (RPAs). A device shares its presence key with every peer it pairs with, and advertises

```text
nonce = 6 random bytes, fresh per rotation
tag   = HMAC-SHA256(presence_key, "continuum/rid/v1" || nonce)[..6]
id    = nonce || tag                    (12 bytes, 24 hex chars)
```

Receivers check the tag against each trusted presence key (constant-time) and cache the result. Because the nonce travels with the tag, the clocks don't need to be in sync. Identifiers rotate every 15 minutes by default, and whenever the BLE address or network changes.

Known limits:

- **Replay.** A captured ID can be re-broadcast to fake presence. Features that act on proximity (auto-unlock) must add a live challenge.
- **Revocation.** Unpairing a peer requires rotating the presence key and redistributing it to the remaining peers.
- **Fingerprint.** The fixed BLE service UUID reveals that *a* Continuum device is nearby. This is unavoidable: Android stops BLE scans with the screen off unless a non-empty `ScanFilter` is set.

## BLE beacon

Non-connectable legacy advertisement, one AD structure:

```text
len(1) | 0x21 service data 128-bit (1) | uuid(16) | beacon(13)   = 31 bytes
beacon = version(4 bits) | flags(4 bits) | id(12)
```

- Service UUID: `9b4354e1-15ec-4324-9f00-e9c5549237f7`
- Version: `1`
- Flags: `0x1` pairing mode (ID is random, not resolvable), `0x2` also reachable on LAN

## mDNS

- Service type: `_continuum._tcp.local.`
- Instance name: the current resolvable ID in hex
- TXT: `v=1` plus flags only. No host name, model, or stable ID.

## Pairing mode

Only while the user has a pairing dialog open, a device advertises a **random, unresolvable** ID with the pairing flag set. Receivers surface these as pairing candidates. The pairing exchange (QR / SAS, Checkpoint 1 M2) transfers public identities and presence keys.

## Presence registry

- Merges BLE and LAN sightings per peer.
- Smooths RSSI with an exponential moving average (α = 0.3) and maps it to `Immediate` / `Near` / `Far` bands. Each band has separate enter and exit thresholds, so a signal at the boundary doesn't flap.
- TTLs are 30 s for BLE and 150 s for LAN. LAN presence also ends on an mDNS goodbye, but only once *every* live identifier for that peer has been withdrawn, so rotation does not cause flapping.

## Quick Share compatibility

Discovery only, in `crates/continuum-quickshare`. Quick Share endpoints are foreign and never enter the trust store.

- Linux as **receiver** publishes `_FC9F5ED42C8A._tcp`. The endpoint info contains the device name in plain text, so by default it is published only for 60 s after an Android sender's BLE trigger (UUID `0xFE2C`) is seen.
- Linux as **sender** broadcasts the `0xFE2C` trigger so Android receivers publish their mDNS records.
- The UKEY2 handshake and transfers are Checkpoint 3.

Formats follow [NearDrop's protocol notes](https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md).
