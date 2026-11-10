---
title: Vision & Philosophy
description: The mission, principles, and user-centric philosophy behind Continuum.
---

## What is Continuum?

**Continuum** is an open-source initiative designed to bring seamless, Apple Continuity-grade integration to the **Linux + Android** ecosystem.

For over a decade, Apple users have enjoyed a cohesive cross-device experience: Mac, iPhone, and iPad communicate with zero setup friction. Devices are naturally aware of each other's physical proximity, exchange clipboard contents in real time, stream camera feeds, mirror displays, transfer files with a single tap, and answer cellular calls from the laptop.

In contrast, Linux and Android users face fragmented, single-purpose utilities, cloud-reliant workarounds, or clunky manual pairings. Continuum bridges this divide by delivering an integrated, local-first ecosystem where your Linux desktop/laptop and Android devices feel like two halves of a single, unified computer.

---

## Core Principles

Continuum is built on five non-negotiable principles:

### 1. Local-First & Zero Cloud Dependency
Every interaction—whether detecting a nearby phone, syncing a clipboard snippet, or streaming a 4K camera feed—happens directly between your devices over local wireless and wired links. 
* No user accounts or centralized login servers.
* No data leaves your personal network or physical proximity.
* Completely functional even when your internet connection is down or when working in air-gapped environments.

### 2. Privacy & Security by Default
Physical proximity should never compromise privacy.
* **Rotating Ephemeral Presence**: Proximity signals continuously rotate to prevent third parties from tracking or fingerprinting your physical location.
* **Mutual Zero-Trust Verification**: Devices never communicate beyond initial proximity discovery until explicitly paired via out-of-band verification (such as numeric verification or QR confirmation).
* **End-to-End Encryption**: Every session uses forward-secret encrypted tunnels. Even on an untrusted public Wi-Fi network, your communications remain impenetrable.

### 3. Frictionless & Ambient
Technology should disappear into the background.
* You should not need to launch an app, click "Connect", or scan for devices before copying text or answering a call.
* As soon as your phone sits on your desk next to your laptop, the devices ambiently know they are together and make their capabilities available.

### 4. Desktop & Platform Neutrality
Continuum embraces the diversity of the Linux ecosystem.
* Works seamlessly across major desktop environments (GNOME, KDE Plasma, Sway, Hyprland, XFCE, and minimal window managers).
* Integrates with modern desktop standards: native system notifications, system trays, desktop portals, Wayland compositors, and sound servers.

### 5. Extensibility & Power-User Freedom
Unlike closed commercial ecosystems that lock down workflows, Continuum is designed to be inspected, scripted, and customized:
* Exposes CLI commands and hooks for every feature.
* Supports arbitrary multi-device meshes (e.g. 1 laptop + 2 tablets + 1 phone + 1 home server).
* Allows granular per-device permission controls.
