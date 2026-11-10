---
title: Feature Matrix & Comparison
description: Comparing Apple Continuity features with Continuum equivalents and beyond.
---

Continuum maps every major Apple Continuity feature to an open, local-first Linux + Android equivalent—and introduces unique augmentations enabled by open platforms.

## Feature Mapping Overview

| Apple Continuity Feature | Continuum Feature Equivalent | Target Experience | Continuum Augmentation (Beyond Apple) |
| :--- | :--- | :--- | :--- |
| **Proximity Awareness** | Ambient Presence Engine | Devices detect nearby state automatically via low-power triggers. | Radar distance meter & "Find My" loud paging from desktop CLI/tray. |
| **AirDrop** | P2P File Transfer | Zero-config, ad-hoc, multi-file and directory transfer at full Wi-Fi speed. | Headless server receivers, CLI pipeline piping (`cat file \| continuum send`), folder watch folders. |
| **Universal Clipboard** | Universal Clipboard | Real-time copy & paste of plain text, rich text, and images across devices. | Multi-item clipboard history buffer, regex filters, and per-device sync toggles. |
| **Continuity Camera** | Continuity Camera | Android camera used as a wireless HD webcam and microphone for desktop apps. | Multi-camera simultaneous streams (front + rear), raw video feeds, and custom framerates. |
| **AirPlay to Mac** | Media Cast Receiver | Cast audio, video, and photos from phone to Linux display and speakers. | Multi-room broadcast, stream capture as OBS video input. |
| **iPhone Mirroring** | Screen Mirroring | Low-latency, interactive phone display window on Linux with keyboard/mouse control. | Multi-window app streaming: launch Android apps into separate native floating desktop windows. |
| **Sidecar** | Virtual Display & Tablet | Android phone/tablet acts as an extended or mirrored monitor with stylus digitizer support. | Unlimited virtual screens, custom resolutions/aspect ratios, orientation lock. |
| **Universal Control** | Cross-Device Pointer | Move your laptop mouse off the screen edge directly into your Android device. | Grid layout customizer, seamless multi-device flow (laptop + multiple tablets/phones). |
| **Continuity Sketch / Markup** | Quick Sketch & Annotation | Prompt phone to draw a sketch or sign a document with live desktop sync. | Vector SVG export, direct integration with Linux creative tools (GIMP, Krita). |
| **iPhone Cellular Calls** | Cellular Call Relay | Incoming call pop-ups on laptop; answer and talk handsfree through laptop mic/speakers. | Call recording with permission, automated audio routing per headset. |
| **Shared Notifications** | Two-Way Notification Hub | Mirror notifications with inline quick-reply and remote action buttons. | Granular per-app filtering, regex mute rules, two-way dismissal sync. |
| **SMS 2FA Auto-Fill** | OTP Verification Auto-Fill | Incoming 2FA codes are automatically detected and copied or suggested on desktop. | One-click auto-fill, regex pattern recognition for banking/custom SMS. |
| **Auto Unlock** | Proximity Auto-Unlock | Unlock Linux screen lock automatically when trusted phone is physically near. | Cryptographic challenge-response preventing replay, configurable distance threshold. |
| **Apple Watch Sudo Approval** | Biometric PAM Sudo Approval | Use Android fingerprint/face unlock to authorize `sudo` commands and password prompts. | Customizable Linux PAM module, timeout rules, per-command policies. |
| **Wi-Fi Password Share** | Wi-Fi Credential Share | One-tap sharing of saved Wi-Fi network credentials to nearby paired devices. | Two-way sync with desktop NetworkManager and Android Wi-Fi store. |
| **Instant Hotspot** | Remote Hotspot Control | Activate phone's Wi-Fi hotspot directly from Linux network menu without touching phone. | Battery & data limit guards, automatic disconnect on idle. |
| **iPhone Widgets on Mac** | Desktop Android Widgets | Display interactive phone home screen widgets directly on Linux desktop. | Resizable native desktop widgets, Wayland status bar integration. |
| **Handoff** | Application & Tab Handoff | Continue active browser tabs and open documents across devices. | Cross-browser compatibility (e.g. Firefox Android <-> Chrome Linux, Brave, etc.). |
