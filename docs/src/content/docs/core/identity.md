---
title: Cryptographic Identity & Trust Store
description: Mutual device verification, zero-cloud pairing, and permission management.
---

Continuum replaces centralized cloud accounts (like Apple ID or Google accounts) with direct, device-to-device cryptographic relationships.

## User Experience

### 1. One-Time Out-of-Band Pairing
* **Effortless Pairing Flow**:
  1. Bring a new Android device near your Linux machine.
  2. Select "Pair New Device" from the Continuum desktop panel.
  3. A 6-digit numeric verification code (or animated QR code) appears simultaneously on both screens.
  4. Confirm that the numbers match (or scan the QR code with your phone camera).
  5. The devices are now permanently and mutually trusted.
* **Man-in-the-Middle (MitM) Prevention**: The short verification string proves cryptographically that no third party is intercepting the connection.

### 2. The Local Trust Store
* Paired devices are stored in a secure local keyring on each device.
* Once paired, devices recognize each other automatically every time they come into proximity—no re-pairing or login prompts required.
* Devices can be renamed, granted custom permissions, or revoked with a single click at any time.

### 3. Dynamic Capability Negotiation
* When two devices establish contact, they exchange a signed manifest of their available capabilities:
  * *"I support HD Camera Streaming v1, Universal Clipboard v2, and Screen Mirroring v1."*
* If one device is running a newer version of Continuum, both devices negotiate the highest mutually supported feature set without breaking compatibility.

---

## Continuum Augmentations (Beyond Apple)

* **Multi-User Trust Tiers**: Create separate trust levels per device (e.g., *Full Trust*, *Read-Only Trust*, or *Guest Transfer Only*).
* **Exportable Encrypted Trust Bundles**: Easily back up or migrate trusted device pairings when formatting your OS or switching machines without requiring cloud sync.
* **Zero Account Lock**: A single Linux machine can be paired with work phones and personal phones simultaneously, keeping their data silos completely isolated.
