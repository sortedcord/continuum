---
title: Proximity Unlock & Roaming Passkeys
description: Walk-away locking, auto-unlock on approach, and hardware FIDO2 passkey authentication.
---

Continuum enhances physical desktop security while eliminating unnecessary password friction.

## 1. Proximity Auto-Unlock & Walk-Away Lock

### User Experience
* **Auto-Unlock on Approach**:
  * As you approach your locked Linux workstation with your trusted Android phone in your pocket, the lock screen recognizes your presence.
  * The screen automatically unlocks—no need to type your password every time you sit down.
* **Walk-Away Auto-Lock**:
  * When you get up from your desk and walk away (e.g., more than 4–5 meters), the system detects the drop in signal strength.
  * Your Linux screen immediately locks itself, protecting your open code, documents, and terminals from shoulder-surfers or unauthorized physical access.

### Security Guarantees
* **RSSI Calibration**: You can calibrate the exact proximity distance threshold in Continuum settings to match your office environment.
* **Cryptographic Nonce Verification**: Proximity alone is not enough; the lock screen exchanges an encrypted cryptographic challenge with the phone to prevent signal spoofing.

---

## 2. Roaming FIDO2 / Passkey Authenticator

Modern web security is moving to passwordless Passkeys (FIDO2 / WebAuthn).

### User Experience
* When logging into a website (GitHub, Google, banking, cloud dashboards) on your Linux desktop browser:
  * Select **"Use Passkey / Mobile Device"**.
  * A passkey authentication prompt appears on your Android phone.
  * Scan your fingerprint on your phone.
  * You are logged into the website on your desktop instantly without typing passwords or retrieving authenticator codes.

---

## Continuum Augmentations (Beyond Apple)

* **Multi-Device Lock Quorum**: Require both your phone AND your smartwatch to be nearby for auto-unlock in high-security environments.
* **Instant Lock Hotkey on Phone**: An ambient persistent tile in Android Quick Settings lets you tap **"Emergency Lock PC"** to instantly lock your desktop from across the room if you forgot to lock it.
