# Continuum — Agent Guidelines & Project Architecture

## 1. Project Vision

**Continuum** is an open-source initiative designed to bring seamless, Apple Continuity-grade integration to the **Linux + Android** ecosystem.

In the Apple ecosystem, devices communicate with zero user friction: they know when they are near each other, establish secure peer-to-peer connections automatically, and expose high-bandwidth, low-latency services (such as AirDrop, Universal Clipboard, iPhone Screen Mirroring, Continuity Camera, Sidecar, and Universal Control).

Continuum aims to replicate this unified device experience between Linux desktops/laptops and Android mobile devices with:
- **Local-first & Zero Cloud Dependency**: All discovery, pairing, and streaming happen locally over peer-to-peer or local network connections.
- **Privacy & Security by Design**: Hardware-backed identity, mutual cryptographic authentication, rotating ephemeral identifiers, and end-to-end encrypted tunnels.
- **Low-Latency & High Performance**: Efficient transport switching (BLE triggers, mDNS/LAN discovery, high-throughput Wi-Fi P2P / WebRTC / QUIC / TCP data channels).
- **Desktop Environment & Platform Neutrality**: Clean core protocol engine with native bindings for modern Linux stacks (PipeWire, V4L2, Wayland/X11, BlueZ, NetworkManager, PAM, Secret Service/Keyring) and standard Android APIs.

---

## 2. High-Level System Architecture

Continuum is structured as a **layered, modular architecture**. High-level features do not implement raw networking or device discovery themselves; instead, they sit atop a robust discovery, security, and transport core.

```
+---------------------------------------------------------------------------------------------------------+
|                                        Application & Feature Layer                                      |
|                                                                                                         |
|  [ AirDrop / Files ]     [ Universal Clipboard ]   [ Continuity Camera/Mic ]  [ Media Cast Receiver ]   |
|  P2P high-speed share    Encrypted text/image sync V4L2 / PipeWire feed       Audio & video streaming   |
|                                                                                                         |
|  [ Screen Mirroring ]    [ Sidecar / 2nd Display ] [ Universal Control ]      [ Sketch & Markup ]       |
|  MediaProjection viewer  Virtual Wayland monitor   Cross-device mouse/kbd     Live drawing & annotation |
|                                                                                                         |
|  [ Call / Notif Relay ]  [ PAM / Sudo Biometrics ] [ Auto-Unlock & Passkey ]  [ Hotspot & Wi-Fi Share ] |
|  Two-way sync & reply    Fingerprint auth for sudo Proximity lock / WebAuthn  Zero-touch net onboarding |
+---------------------------------------------------------------------------------------------------------+
                                                     |
+---------------------------------------------------------------------------------------------------------+
|                                    Session & Protocol Dispatcher Layer                                  |
|  - Capability negotiation & service multiplexing (Protobuf / CBOR schemas)                              |
|  - Dynamic channel selection (BLE / Local Wi-Fi / Wi-Fi Direct / USB fallback)                          |
|  - Stream prioritization, flow control & congestion management                                          |
+---------------------------------------------------------------------------------------------------------+
                                                     |
+---------------------------------------------------------------------------------------------------------+
|                                       Security & Trust Layer (Core)                                     |
|  - Device Identity (Ed25519 / X25519 keypairs per device)                                               |
|  - Out-of-Band Pairing & Verification (QR code / SAS numeric comparison, MitM protection)               |
|  - Mutual TLS / Noise Protocol session encryption (Perfect Forward Secrecy)                            |
|  - Keystore & Secret Service / Keyring integration                                                      |
+---------------------------------------------------------------------------------------------------------+
                                                     |
+---------------------------------------------------------------------------------------------------------+
|                                  Discovery & Presence Engine ("The Core")                               |
|  - Proximity Beaconing & Detection (Bluetooth Low Energy / BLE with rotating ephemeral IDs)             |
|  - Local Subnet Discovery (mDNS / DNS-SD & UDP Multicast)                                               |
|  - Device State & Reachability Tracking (Heartbeats, RSSI radar, and Find My Device paging)             |
+---------------------------------------------------------------------------------------------------------+
                                                     |
+---------------------------------------------------------------------------------------------------------+
|                                          Hardware & OS Abstraction                                      |
|  Linux: BlueZ, D-Bus, Avahi, PipeWire, V4L2, Wayland/evdev/uinput, PAM, NetworkManager, Secret Service |
|  Android: BLE Scanner/Advertiser, NSD, CameraX, MediaProjection, BiometricPrompt, InputAccessibility    |
+---------------------------------------------------------------------------------------------------------+
```

---

## 3. Checkpoint 1: The Core Discovery Engine (Foundational Milestone)

The immediate objective of the project is **Checkpoint 1: The Core Discovery System**.

Every Continuity feature relies on answering three fundamental questions without user intervention:
1. *Who is nearby?* (Presence & Proximity Detection)
2. *Is it a trusted device?* (Cryptographic Identity & Mutual Authentication)
3. *What is the best route to talk to it?* (Connection Orchestration & Capability Negotiation)

### Core Subsystems to Build in Checkpoint 1

1. **Multi-Modal Discovery Service**
   - **Proximity Tier (BLE)**: Low-power advertising and scanning using Bluetooth Low Energy. Broadcasts ephemeral, rotating identifiers to detect immediate physical proximity without battery drain or privacy leakage.
   - **Local Network Tier (mDNS / DNS-SD)**: High-speed discovery over shared Wi-Fi/Ethernet subnets when both devices reside on the same network.
   - **Presence Registry**: Aggregates discovered signals, resolves duplicates, tracks signal strength (RSSI), network addresses, and maintains real-time online/nearby state.
   - **Proximity Paging ("Find My Device")**: Ability to trigger an audible ring on Android from Linux (overriding silent mode) and gauge distance via BLE RSSI radar.

2. **Device Identity & Pairing Subsystem**
   - Cryptographic identity generation per device (Ed25519 for signing, X25519 for key exchange).
   - Pairing workflow: Out-of-band verification (Short Authentication String / QR code scan) to prevent Man-in-the-Middle (MitM) attacks.
   - Trust Store: Persistent, secure storage of paired peer keys and permissions.

3. **Secure Handshake & Capability Exchange**
   - Lightweight initial handshake over the best available transport (BLE or TCP/TLS or Noise protocol).
   - Device capability advertisement: exchanging supported service manifests (e.g., `camera.v1`, `clipboard.v1`, `sidecar.v1`, `file-transfer.v2`).
   - Keepalive, connection health monitoring, and graceful disconnect detection.

---

## 4. Full Project Roadmap

```
+---------------+      +---------------+      +---------------+
| Checkpoint 1  | ---> | Checkpoint 2  | ---> | Checkpoint 3  |
| Core Discovery|      | Pairing, Clip-|      | AirDrop &     |
| & Presence    |      | board, 2FA/Net|      | Sketch/Markup |
+---------------+      +---------------+      +---------------+
                                                     |
                                                     v
+---------------+      +---------------+      +---------------+
| Checkpoint 6  | <--- | Checkpoint 5  | <--- | Checkpoint 4  |
| PAM, Handoff, |      | Screen Mirror,|      | Camera, Mic & |
| Unlock & Pass |      | Sidecar, Ctrl |      | Cast Receiver |
+---------------+      +---------------+      +---------------+
```

### Detailed Milestone Breakdown

#### Checkpoint 1: The Core Discovery Engine & Trust Architecture
- BLE proximity beaconing with rotating ephemeral identifiers.
- mDNS / DNS-SD local network presence detection.
- Presence registry with RSSI tracking and reachability state.
- Device identity generation and persistent trust store.
- Out-of-band mutual authentication pairing (SAS numeric comparison / QR code).
- Service capability negotiation framework and keepalive monitoring.
- Proximity paging ("Find My Device" audible ring and RSSI radar).

#### Checkpoint 2: Secure Transport, Universal Clipboard & Quick Data Exchange
- End-to-end encrypted session channels (Mutual TLS / Noise Protocol with Perfect Forward Secrecy).
- **Universal Clipboard**:
  - Bidirectional clipboard synchronization (plain text, HTML/rich text, images).
  - Privacy safeguards (sensitive data filtering, clipboard history limits, pause/resume toggles).
- **Wi-Fi Credential Sharing**:
  - Zero-friction sharing of saved Wi-Fi networks (NetworkManager <-> Android Wi-Fi) when a paired device detects an unknown nearby network.
- **SMS 2FA OTP Auto-Fill**:
  - Automatic detection of incoming one-time passwords (OTPs) from Android SMS, auto-suggesting or copying them directly to the Linux clipboard.

#### Checkpoint 3: High-Speed File Transfer & Document Continuity (AirDrop Equivalent)
- **AirDrop Equivalent (High-Speed File Transfer)**:
  - Zero-config peer-to-peer file send and receive.
  - Multi-file and directory transfer support.
  - Chunked streaming, checksum verification, and resumable transfers over local Wi-Fi / Wi-Fi Direct.
  - Native Linux desktop notifications, file manager integration, and Android share sheet support.
- **Continuity Sketch & Markup**:
  - Trigger drawing or PDF annotation sessions on Android directly from Linux.
  - Live vector/bitmap syncing back to Linux graphic editors and document viewers.

#### Checkpoint 4: Continuity Camera, Microphone & Media Cast (AV Stream Engine)
- **Continuity Camera**:
  - Low-latency, hardware-accelerated video streaming from Android camera (CameraX/Camera2) to Linux.
  - Virtual webcam device on Linux (via V4L2 loopback or PipeWire virtual camera).
  - Desktop camera controls (lens switching, exposure, flash, resolution).
  - Instant document scanning and photo capture triggers.
- **Continuity Microphone**:
  - High-fidelity wireless audio streaming from phone mic to Linux audio subsystem (PipeWire / PulseAudio source).
- **Media Cast Receiver (AirPlay Equivalent)**:
  - Linux operating as an audio/video cast receiver for media streamed directly from Android.

#### Checkpoint 5: Screen Mirroring, Virtual Displays & Universal Input
- **Phone Screen Mirroring**:
  - Android screen capture (MediaProjection) with hardware H.264/H.265/AV1 encoding.
  - Desktop viewing window with ultra-low latency rendering.
  - Forwarding touch/click, gestures, scroll, keyboard shortcuts via accessibility or Linux `uinput`/`evdev`.
  - Synchronized Android audio forwarding to Linux speakers.
- **Sidecar Equivalent (Second Monitor & Graphics Tablet)**:
  - Use an Android tablet or phone as an extended or mirrored Linux display via virtual Wayland/DRM outputs.
  - Pressure-sensitive stylus and digitizer input forwarding to Linux applications (Krita, GIMP, Blender).
- **Universal Control Equivalent (Cross-Device Mouse & Keyboard)**:
  - Seamlessly push the Linux mouse cursor off the edge of the monitor onto the Android screen.
  - Share single mouse and keyboard focus across physical devices without screen mirroring.

#### Checkpoint 6: Deep Ecosystem, Productivity & Security Integration
- **PAM / Sudo Biometric Approval (`pam_continuum`)**:
  - Approve Linux `sudo` commands and PolicyKit/Keyring privilege elevation prompts using the Android phone's biometric sensor (fingerprint/face unlock).
- **Proximity Auto-Unlock**:
  - Automatically unlock the Linux desktop when a trusted Android phone is in close physical range (BLE RSSI + cryptographic challenge).
- **FIDO2 / WebAuthn Roaming Passkey Authenticator**:
  - Use the Android device as a hardware passkey / WebAuthn authenticator for Linux desktop web browsers.
- **Handoff**:
  - Resume active browser tabs or active app context when switching between Linux and Android.
- **Cellular Call Relay & Handsfree Calling (Desktop Call Pickup)**:
  - **Incoming Call Pop-Up**: Real-time incoming call banner on Linux with Caller ID, contact photo, and quick actions: *Answer*, *Decline*, or *Decline with SMS*.
  - **Full-Duplex Audio Handover**: Answering on Linux routes phone call audio in real time directly through your laptop's microphone and speakers/headset via PipeWire/PulseAudio and Android's `TelecomManager`/`InCallService`.
  - **In-Call Desktop Controls**: On-screen call management for mute, hold, audio device switching, DTMF keypad (dial pad for automated phone trees), and seamless one-click call transfer back to the phone.
  - **Outbound Dialing**: Dial phone numbers directly from Linux (e.g. clicking `tel:` links in browsers, emails, or desktop contacts) and place the call handsfree through the phone.
- **Shared Notifications & Bidirectional Action Hub**:
  - **Zero-Latency Notification Mirroring**: Android notifications appear instantly as native Linux desktop notifications (`org.freedesktop.Notifications`) with original app icons and titles.
  - **Inline Quick Reply**: Reply directly to chat messages (SMS/RCS, WhatsApp, Signal, Telegram, Slack) from the desktop notification pop-up without touching your phone.
  - **Remote Action Execution**: Trigger rich notification action buttons directly from Linux ("Mark as Read", "Archive", "Delete", "Snooze", "Like").
  - **Two-Way Dismissal Sync**: Dismissing or swiping away a notification on Linux instantly clears it from the phone's notification shade, and vice-versa.
  - **Live Activities & Ongoing Status**: Pinned status indicators for active timers, media playback, and delivery/ride tracking integrated into the Linux desktop panel or system tray.
  - **Privacy & Filtering Controls**: Per-app notification forwarding toggles, privacy mode (hiding notification contents on lock screen), and synchronized Do Not Disturb (DND) status.
- **Instant Hotspot**:
  - Remotely activate Android Wi-Fi hotspot from Linux network menu without touching the phone.
- **Android Widgets on Linux Desktop**:
  - Display and interact with Android AppWidget remote views directly on the Linux desktop or dashboard.

---

## 5. Guidelines for AI Agents Working on Continuum

When working on this repository, all agents must adhere to the following principles:

### A. Focus on Checkpoint 1 First
- Do not implement end-user features (file transfer, camera streaming, screen mirroring) before the Discovery and Security Core is established.
- All new protocols, interfaces, and abstractions must be designed around the Core's service discovery and session management primitives.

### B. Protocol-First & Transport-Agnostic Design
- Decouple feature logic from transport mechanisms. Features must request a stream or send messages through the Core session abstraction, regardless of whether the physical path is Wi-Fi, BLE, Wi-Fi Direct, or USB.
- Use explicit, versioned protocol schemas (e.g., Protobuf, CBOR, or typed JSON messages) for all inter-device communications.

### C. OS Isolation & Modularity
- Isolate platform-specific APIs behind clean interfaces:
  - Linux specifics (D-Bus, BlueZ, PipeWire, V4L2, PAM, Wayland/uinput, Secret Service) must remain in Linux adapter layers.
  - Android specifics (BLE Central/Peripheral, NSD, CameraX, MediaCodec, Biometrics, Accessibility) must remain in Android adapter layers.
  - Core logic (state machines, cryptographic handshakes, routing) should remain portable and testable independently.

### D. Security by Default
- Never transmit unauthenticated or unencrypted payloads over the network.
- Every discovered device is treated as untrusted until validated through the trust store and mutual handshake.
- Ephemeral identifiers must rotate to prevent physical tracking via BLE or mDNS sniffing.

### E. Code Quality, Testing & Mocks
- Write comprehensive unit tests for protocol parsing, handshake state machines, and trust verification.
- Provide mock transport layers (in-memory or virtual sockets) so discovery and feature logic can be tested without requiring physical Bluetooth or Wi-Fi hardware in CI/test environments.
