---
title: Discovery Protocol (Spec)
description: How Continuum devices find each other without leaking who they are.
---

Status: **draft, Checkpoint 1.** Reference implementation in `crates/continuum-core` (sans-I/O logic), `crates/continuum-proto` (wire formats), `crates/continuum-net` (async transport) and `crates/continuum-linux` (Avahi, BlueZ, Secret Service).

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

Only while the user has a pairing dialog open, a device advertises a **random, unresolvable** ID with the pairing flag set (BLE and mDNS). Receivers surface these as pairing candidates.

## Connections

Every TCP connection starts with a 6-byte preamble, `"CNTM" | version(1) | intent(1)`, followed by frames of `u16 big-endian length | bytes`. Handshake messages and encrypted transport messages are one frame each (at most 65 535 bytes).

| Intent | Noise pattern | Authenticated by |
| --- | --- | --- |
| `1` session | `Noise_IK_25519_ChaChaPoly_BLAKE2s` | Static keys from the trust store |
| `2` pair with QR secret | `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` | 32-byte one-time secret from the QR code |
| `3` pair by code | `Noise_XX_25519_ChaChaPoly_BLAKE2s` | Users compare a 6-digit SAS |

The prologue is `continuum/v1`. The Noise static key is the device's X25519 agreement key.

### Pairing

1. The device being paired shows an invite: `continuum:pair:` + base64url(`PairingInvite` protobuf) carrying its LAN addresses, a one-time 32-byte secret, its name and an expiry (5 minutes by default). About 125 characters, so the QR code stays small.
2. The joiner connects and runs `XXpsk3` with the secret. A wrong secret fails the handshake.
3. Each side sends a `PairingRecord`: Ed25519 key, X25519 key, presence key, device info, and an Ed25519 signature over `"continuum/pair/v1" || handshake_hash || agreement_key || presence_key`. The receiver checks that the agreement key equals the Noise remote static key and verifies the signature. Signing the handshake hash binds the record to this exact handshake, so it can't be replayed elsewhere.
4. In the code flow, both users must confirm the SAS: `SHA-256("continuum/sas/v1" || handshake_hash)`, first 8 bytes as a big-endian integer, mod 1 000 000.
5. Both sides send `PairingResult`. The peer is stored only if both accepted.

### Sessions

The initiator already knows the responder's static key from the trust store (`IK`, one round trip). The responder looks up the initiator's static key after the first message and drops unknown initiators **before replying**, so strangers learn nothing. Both sides then exchange `Hello` (device info plus capability manifest) inside an `Envelope`.

The session sends a `Ping` every 10 s and closes after 3 silent intervals. If both devices connect at once, both keep the session initiated by the lower `DeviceId`. The higher-id device waits 10 s before connecting itself.

## Storage

On Linux, the identity secrets (96 bytes) and the trust store (`StoredTrust` protobuf, which includes peer presence keys) are two Secret Service items, with attributes `application=continuum`, `kind=identity|trust` and `profile=<name>`.

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
