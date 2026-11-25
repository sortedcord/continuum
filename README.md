# Continuum

<div align="center">

**Seamless, Apple Continuity-Grade Integration for the Linux + Android Ecosystem.**

*Local-First • Zero Cloud Dependencies • Privacy by Design • Platform Neutral*

[![Documentation](https://img.shields.io/badge/docs-Starlight-blue.svg)](https://sortedcord.github.io/)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

</div>

---

## 1. Overview

In the Apple ecosystem, devices communicate with zero user friction: they know when they are near each other, establish secure peer-to-peer connections automatically, and expose high-bandwidth, low-latency services (such as AirDrop, Universal Clipboard, iPhone Screen Mirroring, Continuity Camera, Sidecar, and Universal Control).

Linux and Android users, however, are left with fragmented single-purpose tools, cloud-reliant workarounds, or clunky manual pairing workflows.

**Continuum** bridges this divide by delivering an integrated, ambient cross-device bridge where your Linux workstation and Android phone/tablet behave like a unified, cohesive computing environment.

---

## 2. Core Philosophy

* **Local-First & Zero Cloud**: All discovery, pairing, file transfers, and streaming happen directly between your devices over local wireless links (BLE, Wi-Fi, Wi-Fi Direct). No accounts, no servers, and zero cloud tracking.
* **Privacy by Default**: Proximity signals use rotating ephemeral identifiers to prevent tracking. Communications use end-to-end forward-secret encrypted tunnels.
* **Ambient & Frictionless**: Devices recognize each other's physical proximity automatically. No need to open apps, scan barcodes, or click "Connect" every time you sit at your desk.
* **Desktop Neutrality**: Designed to integrate cleanly into modern Linux desktop environments (GNOME, KDE Plasma, Sway, Hyprland, XFCE) and modern Linux system stacks.

---

## 3. Feature Matrix

Continuum maps every major Apple Continuity feature to an open equivalent—and introduces unique augmentations enabled by open platforms:

| Apple Continuity Feature | Continuum Feature Equivalent | Description | Continuum Superpower (Beyond Apple) |
| :--- | :--- | :--- | :--- |
| **Proximity Awareness** | **Ambient Presence Engine** | Devices detect proximity automatically via low-power BLE triggers. | Distance radar & loud audible paging from desktop CLI/tray. |
| **AirDrop** | **P2P File Transfer** | Zero-config, ad-hoc, multi-file and directory transfer at full Wi-Fi speed. | Headless server receivers, CLI pipeline piping (`cat file \| continuum send`), drop folders. |
| **Universal Clipboard** | **Universal Clipboard** | Instant copy & paste of plain text, rich formatting, and images across devices. | Multi-item history buffer, regex transformers, password-manager exclusions. |
| **Continuity Camera** | **Continuity Camera** | Android camera used as a wireless HD webcam for desktop conferencing. | Multi-camera simultaneous streams (front + rear), raw video passthrough. |
| **Continuity Mic** | **Continuity Microphone** | Use phone's multi-mic array as a high-fidelity wireless desktop microphone. | Low-latency Opus streaming, studio voice isolation modes. |
| **AirPlay to Mac** | **Media Cast Receiver** | Cast videos, music, and photos from phone to Linux display and speakers. | OBS Studio integration as video capture source, multi-room audio sync. |
| **iPhone Mirroring** | **Screen Mirroring** | Low-latency interactive phone display window on Linux with keyboard/mouse control. | **Multi-window app streaming**: run individual Android apps as separate floating desktop windows. |
| **Sidecar** | **Virtual Display & Tablet** | Use phone/tablet as a virtual second monitor with stylus digitizer support. | Unlimited virtual screens, custom resolutions/aspect ratios, pressure & tilt support. |
| **Universal Control** | **Cross-Device Cursor** | Push your desktop mouse pointer off the monitor edge directly into Android. | Custom spatial grid layout, multi-machine flow (1 laptop + multiple tablets/phones). |
| *(No Native Apple Equivalent)* | **Virtual Trackpad & Gamepad** | Use phone screen as a multi-touch trackpad, remote keyboard/numpad, presentation air-mouse, or wireless gamepad. | Multi-player virtual gamepads (connect 4 phones for split-screen gaming), custom macro pads (Stream Deck alternative), gyro steering. |
| **Continuity Sketch** | **Quick Sketch & Markup** | Draw diagrams, sign PDFs, and annotate documents on mobile with live desktop sync. | Resolution-independent vector SVG exports, direct integration with GIMP/Krita. |
| **Cellular Calls** | **Cellular Call Relay** | Incoming call pop-ups on laptop; answer and talk handsfree via laptop mic/speakers. | In-call controls, DTMF keypad, local call recording with consent. |
| **Shared Notifications** | **Two-Way Notification Hub** | Mirrored notifications with inline quick-reply and remote action buttons. | Full two-way dismissal sync, per-app filtering, regex mute rules. |
| **SMS 2FA Auto-Fill** | **OTP Verification Auto-Fill** | Verification codes in incoming SMS are auto-detected and copied/suggested on desktop. | Cross-browser auto-fill, terminal SSH suggestion, custom regex support. |
| **Auto Unlock** | **Proximity Auto-Unlock** | Unlock Linux screen lock automatically when trusted phone is physically near. | Walk-away auto-lock, distance threshold calibration, replay-safe crypto challenge. |
| **Apple Watch Approval**| **Biometric PAM Sudo Approval**| Approve `sudo` commands and admin prompts using phone's fingerprint sensor. | Configurable Linux PAM module (`pam_continuum`), per-command security policies. |
| **Wi-Fi Password Share** | **Wi-Fi Credential Share** | One-tap sharing of saved Wi-Fi network credentials to nearby companion devices. | Two-way sync with desktop NetworkManager and Android Wi-Fi store. |
| **Instant Hotspot** | **Remote Hotspot Control** | Activate phone's Wi-Fi hotspot directly from Linux network menu without touching phone. | Battery & cellular data limit guards, automatic power-down on idle. |
| **iPhone Widgets on Mac**| **Desktop Android Widgets** | Display and interact with phone home screen widgets directly on Linux desktop. | Native Wayland panel/bar integration, resizable desktop widgets. |
| **Handoff** | **Application Handoff** | Continue active browser tabs and open documents across devices. | Cross-browser compatibility (e.g. Chrome Android <-> Firefox Linux). |

---

## 4. Continuum Superpowers (Beyond the Walled Garden)

1. **N-Device Peer Mesh**: Unlike Apple's strict 1:1 single-account model, Continuum forms an ad-hoc local mesh across multiple laptops, phones, tablets, and headless servers.
2. **UNIX CLI-First**: Every feature is scriptable from your terminal (`continuum send`, `continuum clipboard`, `continuum ping`, `continuum call`).
3. **Multi-Window App Streaming**: Stream individual Android applications into separate native floating desktop windows instead of mirroring the entire phone display.
4. **Biometric PAM Sudo Integration**: Authorize terminal `sudo` commands or desktop authentication prompts with a quick touch on your phone's fingerprint scanner.
5. **Zero Cloud Enforcement**: Fully operational in air-gapped environments or local offline Wi-Fi hotspots with zero external accounts.
6. **Granular Privacy Matrix**: Configure precise per-device permissions (e.g., allow clipboard sync to your work laptop, but restrict notifications).

---

## 5. Roadmap

The project progresses through six modular checkpoints:

* **Checkpoint 1: The Core Discovery Engine (Current Focus)**
  * Ambient BLE beaconing and mDNS/DNS-SD local presence.
  * Rotating ephemeral identifiers and presence registry.
  * Cryptographic identity generation and out-of-band mutual pairing (QR / SAS).
  * Capability negotiation handshake and "Find My Device" proximity paging.
* **Checkpoint 2: Secure Transport, Universal Clipboard & Quick Data Sync**
  * Forward-secret encrypted channels (Mutual TLS / Noise Protocol).
  * Universal Clipboard with password manager exclusions.
  * Wi-Fi credential sharing and SMS 2FA OTP auto-detection.
* **Checkpoint 3: High-Speed File Transfer & Document Continuity**
  * AirDrop equivalent (multi-file, directory, and resumable P2P streaming).
  * Continuity Sketch & Markup with vector SVG sync.
* **Checkpoint 4: Continuity Camera, Microphone & Media Cast**
  * HD wireless webcam feed and desktop camera controls.
  * Wireless microphone audio stream.
  * Media Cast receiver for video and audio playback.
* **Checkpoint 5: Screen Mirroring, Virtual Displays & Universal Input**
  * Low-latency phone screen mirroring with remote input and audio forwarding.
  * Sidecar virtual second monitor and stylus graphics tablet mode.
  * Universal Control cross-device mouse and keyboard sharing.
  * Virtual multi-touch trackpad, remote keyboard/numpad, air mouse, and wireless gamepad.
* **Checkpoint 6: Deep Ecosystem, Productivity & Security Integration**
  * Cellular call relay and full-duplex handsfree calling.
  * Shared notifications with inline reply and two-way dismissal sync.
  * Biometric PAM module (`pam_continuum`) for sudo authorization.
  * Proximity Auto-Unlock and FIDO2 roaming passkey authenticator.
  * Handoff, Remote Instant Hotspot, and Android desktop widgets.

---

## 6. Building & Running (Checkpoint 1 preview)

### Layout

| Path | What it is |
| :--- | :--- |
| `crates/continuum-proto` | Versioned wire formats (BLE beacon, Protobuf messages) |
| `crates/continuum-core` | Portable, I/O-free core: identity, rotating ids, presence registry, Noise handshakes, pairing |
| `crates/continuum-net` | Async framing, pairing and encrypted sessions over any byte stream |
| `crates/continuum-node` | Portable node: rotation, presence and session management |
| `crates/continuum-quickshare` | Quick Share discovery compatibility |
| `crates/continuum-linux` | Linux adapters: Avahi (mDNS), BlueZ (BLE), Secret Service (keys) |
| `crates/continuum-daemon` | The `continuum` CLI and daemon |
| `crates/continuum-ffi` | UniFFI bindings used by the Android app |
| `android/` | Android app (Kotlin, Jetpack Compose, minSdk 31) |

### Linux

Needs Rust 1.89+, `protoc`, and running `avahi-daemon`, `bluetoothd` and a Secret Service provider (GNOME Keyring, KWallet).

```bash
cargo build --release -p continuum-daemon
./target/release/continuum pair invite   # show a QR code to pair a phone
./target/release/continuum run           # presence daemon
./target/release/continuum peers
```

Two Linux machines can pair with `continuum pair join '<invite>'` or `continuum pair scan` (code comparison).

### Android

Needs the Android SDK (`ANDROID_HOME`), NDK 29, `cargo-ndk`, and the Rust targets `aarch64-linux-android` and `x86_64-linux-android`. Gradle builds the Rust library and generates the Kotlin bindings automatically:

```bash
cd android
./gradlew installDebug
```

In the app, tap **Pair device** and scan the QR code from `continuum pair invite`.

---

## 7. Documentation Site

The full documentation is built with [Starlight](https://starlight.astro.build/) and [Astro](https://astro.build/).

### Running the Docs Locally

Ensure you have Node.js (v20+) installed:

```bash
# Clone the repository
git clone https://github.com/sortedcord/continuum.git
cd continuum

# Install docs dependencies
cd docs
npm install

# Start the development server
npm run dev
```

Open [http://localhost:4321/](http://localhost:4321/) in your browser.

To build the static documentation:

```bash
npm run build
```

The production output will be generated in `docs/dist/`.

---

## 8. License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
