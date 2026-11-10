---
title: Ambient Presence & Proximity
description: Seamless zero-friction discovery and awareness between nearby devices.
---

The core foundation of the Continuum experience is **Ambient Presence**: devices automatically sensing when they are physically near each other, without requiring the user to open apps, tap "Scan", or toggle connection switches.

## User Experience

### 1. Zero-Friction Awareness
* When you sit down at your desk with your phone in your pocket or on the charging pad, your laptop silently registers that your phone is nearby.
* System menus (like the panel tray or quick settings) quietly illuminate to indicate available companion devices.
* Services (like camera feeds, clipboard sync, and call audio) immediately become ready for instant invocation.

### 2. Dual-Tier Range Sensing
* **Immediate Physical Proximity**: Sensing devices within arm's reach (room-level or desk-level) using ultra-low-power radio triggers.
* **Local Subnet Range**: Detecting companion devices active on the same office or home Wi-Fi/Ethernet network, enabling high-bandwidth transfers even if devices are in different rooms.

### 3. Presence Radar & "Find My Device" Paging
* **Proximity Radar**: The desktop UI shows an ambient signal meter indicating whether your phone is getting closer or moving farther away.
* **Audible Paging**: A single click from the Linux desktop panel or terminal sends an immediate audible chirp or ring command to your phone, overriding "Do Not Disturb" and silent mode so you can locate a misplaced device in seconds.

---

## Privacy by Design

* **Rotating Ephemeral Identifiers**: Beacon signals constantly randomize their identifiers to prevent eavesdroppers or malicious sniffers from tracking your physical movements or identifying your hardware.
* **Stealth Inactive Mode**: Devices can be set to "Known Contacts Only" or "Completely Hidden" to prevent unsolicited discovery in crowded public spaces like coffee shops and airports.

---

## Continuum Augmentations (Beyond Apple)

* **RSSI Distance Thresholds**: Set customizable actions based on physical distance (e.g., auto-lock your screen when you walk more than 5 meters away; dim the screen when you step back).
* **Location-Aware Mesh Groups**: Automatically group devices based on ambient SSIDs or physical presence (e.g., auto-switching between "Home Devices" and "Office Devices").
* **Terminal Paging**: Locate lost devices directly from a shell script or hotkey (`continuum ping --loud`).
