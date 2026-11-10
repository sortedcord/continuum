---
title: Handoff & Desktop Widgets
description: Seamless task continuity, remote mobile hotspot control, and Android home screen widgets on desktop.
---

Continuum ties your mobile workflow together with intuitive task handoff, remote tethering, and interactive desktop widgets.

## 1. Application & Browser Tab Handoff

### User Experience
* Reading an article or documentation page in a mobile browser on your Android phone during your commute:
  * Open your Linux laptop at your desk.
  * A subtle Handoff badge appears in your desktop dock or application panel: *"Resume Article from Pixel 9"*.
  * Clicking it opens the exact URL immediately in your default desktop browser (Firefox, Chromium, Brave, etc.).
* Works bidirectionally: send an open tab or document from your Linux desktop straight to your phone when you stand up to head out.

---

## 2. Remote Instant Hotspot

Activating a mobile hotspot typically requires taking out your phone, unlocking it, opening settings, toggling hotspot, and waiting for your laptop to scan Wi-Fi networks.

### User Experience
* When you are away from Wi-Fi with your Linux laptop:
  * Open your desktop network menu.
  * Your Android phone appears under a dedicated **"Personal Hotspots"** section showing signal strength (5G/LTE) and phone battery percentage.
  * Click your phone's name.
  * Continuum remotely wakes your phone's hotspot radio, connects your laptop, and establishes the internet connection—all while your phone stays in your pocket or backpack.
* **Smart Power-Down**: When your laptop connects to a known Wi-Fi network or goes to sleep, it automatically signals the phone to turn off the hotspot to save battery.

---

## 3. Android Widgets on Linux Desktop

Apple's macOS Sonoma introduced iPhone widgets on the Mac desktop. Continuum brings this concept to Linux.

### User Experience
* Select and place your favorite Android home screen widgets directly onto your Linux desktop or side panel:
  * Weather widgets, smart home controls, calendar agendas, to-do lists, battery gauges.
* The widgets render natively with crisp high-DPI scaling.
* Tapping a widget on your desktop updates data or triggers the corresponding action on your phone.

---

## Continuum Augmentations (Beyond Apple)

* **Cross-Browser Handoff**: Works seamlessly across different browsers (e.g. hand off from Chrome on Android to Firefox on Linux).
* **Hotspot Data Cap Alerts**: Set a data limit threshold on your laptop to prevent streaming large updates over your phone's cellular connection.
* **Wayland Desktop Panel Integration**: Dock Android widgets directly into custom Wayland status bars like Waybar, Polybar, or GNOME top bars.
