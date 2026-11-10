---
title: Project Roadmap
description: The phased checkpoint progression for Continuum.
---

Continuum follows a structured, modular roadmap where every milestone builds strictly upon the verified primitives of preceding milestones.

```
+--------------------+       +--------------------+       +--------------------+
|   Checkpoint 1     | ----> |   Checkpoint 2     | ----> |   Checkpoint 3     |
|   Discovery Core & |       |   Encrypted Tunnel,|       |   High-Speed Files |
|   Trust Foundation |       |   Clipboard & Sync |       |   & Sketch/Markup  |
+--------------------+       +--------------------+       +--------------------+
                                                                     |
                                                                     v
+--------------------+       +--------------------+       +--------------------+
|   Checkpoint 6     | <---- |   Checkpoint 5     | <---- |   Checkpoint 4     |
|   Calls, Notifs,   |       |   Screen Mirror,   |       |   Camera, Mic &    |
|   PAM & Ecosystem  |       |   Displays & Input |       |   Cast Receiver    |
+--------------------+       +--------------------+       +--------------------+
```

---

## Checkpoint Breakdown

### Checkpoint 1: The Core Discovery Engine (Foundational Priority)
* **Goal**: Establish ambient device awareness, cryptographic identity, and trusted pairing.
* **Key Features**:
  * Dual-tier discovery: low-power Bluetooth Low Energy (BLE) proximity beacons + high-speed local network (mDNS/DNS-SD) presence.
  * Ephemeral rotating identifiers to prevent physical tracking and fingerprinting.
  * Presence registry maintaining real-time reachability, RSSI signal strength, and transport paths.
  * Cryptographic device identity and persistent local trust store.
  * Out-of-band mutual authentication pairing (Short Authentication String / QR code scan).
  * Capability negotiation handshake (manifest of supported services).
  * "Find My Device" proximity paging (audible alarm and RSSI radar).

### Checkpoint 2: Secure Transport, Universal Clipboard & Quick Data Sync
* **Goal**: Enable instantaneous, encrypted text/image copy-pasting and credential sharing.
* **Key Features**:
  * End-to-end encrypted tunnels with Perfect Forward Secrecy.
  * Bidirectional Universal Clipboard (text, rich formatting, images) with sensitive data filters.
  * Wi-Fi Credential Sharing (one-tap sharing of network credentials between devices).
  * SMS 2FA OTP auto-detection and suggestion on desktop.

### Checkpoint 3: High-Speed File Transfer & Document Continuity
* **Goal**: Deliver zero-friction, ultra-fast peer-to-peer file sharing and live document sketching.
* **Key Features**:
  * AirDrop equivalent: zero-config P2P file transfers over local Wi-Fi and Wi-Fi Direct.
  * Multi-file and directory transfer support with progress tracking and checksum verification.
  * Desktop notifications, native file manager integrations, and Android share sheet.
  * Continuity Sketch & Markup: trigger drawing or document annotation on Android with live vector/bitmap sync.

### Checkpoint 4: Continuity Camera, Microphone & Media Cast
* **Goal**: Turn the mobile device into a high-grade wireless audio/video peripheral for desktop.
* **Key Features**:
  * HD wireless webcam feed from phone camera to Linux desktop video applications.
  * Desktop camera controls (lens selection, exposure, flash, resolution).
  * High-fidelity wireless microphone streaming into the Linux audio system.
  * Media Cast Receiver (cast video, music, and photos from phone to Linux display and speakers).
  * Instant document scan and photo capture triggers.

### Checkpoint 5: Screen Mirroring, Virtual Displays & Universal Input
* **Goal**: Unify screens and input peripherals across physical devices.
* **Key Features**:
  * Low-latency phone screen mirroring with remote keyboard, mouse, and gesture control.
  * Audio forwarding with synchronized playback.
  * Sidecar equivalent: use phone/tablet as a virtual second monitor with pressure-sensitive stylus support.
  * Universal Control equivalent: move desktop mouse cursor off screen edge directly onto Android device.

### Checkpoint 6: Deep Ecosystem, Productivity & Security Integration
* **Goal**: Complete the seamless continuity bridge with native operating system hooks.
* **Key Features**:
  * Incoming phone call pop-ups with desktop answer, decline, and full-duplex handsfree audio.
  * Shared notifications with inline quick-reply and two-way dismissal sync.
  * Biometric PAM module (`pam_continuum`) to approve `sudo` commands using phone fingerprint.
  * Proximity Auto-Unlock based on trusted phone range.
  * Passkey / FIDO2 roaming authentication for desktop web logins.
  * Handoff for active browser tabs and application state.
  * Remote Instant Hotspot activation.
  * Android home screen widgets rendered directly on the Linux desktop.
