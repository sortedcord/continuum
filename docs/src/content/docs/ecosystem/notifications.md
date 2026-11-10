---
title: Shared Notifications Hub
description: Real-time notification mirroring, inline quick-replies, and bidirectional dismissal sync.
---

Continuum connects your mobile notifications to your desktop notification system, keeping you informed and responsive without phone distractions.

## User Experience

### 1. Zero-Latency Desktop Mirroring
* Whenever an application on your Android phone posts a notification:
  * It appears instantly on your Linux desktop via the native desktop notification daemon (`org.freedesktop.Notifications`).
  * Features original app icons (WhatsApp, Signal, Slack, Bank, Gmail, etc.), high-contrast text, and sender avatars.
  * Respects your desktop's notification sound and visual styling (GNOME Shell, KDE Plasma, Mako/Dunst on Wayland).

### 2. Inline Quick-Reply
* When receiving incoming chat messages or SMS:
  * Click **"Reply"** directly within the Linux notification pop-up.
  * Type your message and hit `Enter`.
  * The reply is transmitted to your phone and sent immediately through the native messaging app.
  * Works across SMS/RCS, WhatsApp, Signal, Telegram, Discord, Slack, and Matrix.

### 3. Remote Action Buttons
* Android notifications with rich action buttons render those buttons directly on desktop:
  * *"Mark as Read"* for emails.
  * *"Archive"* or *"Delete"* for alerts.
  * *"Snooze"* for calendar reminders.
  * Clicking an action button on your desktop executes the action remotely on your phone.

### 4. Bidirectional Dismissal Sync
* **Clear on Desktop**: Closing, dismissing, or swiping away a notification on your Linux machine instantly clears it from your phone's notification tray.
* **Clear on Phone**: Swiping away a notification on your phone immediately dismisses the corresponding desktop notification banner.

### 5. Live Activities & Persistent Status
* Active ongoing mobile events (such as active timers, ride-share/food delivery progress, or music playing) display as pinned, dynamic widgets in your Linux system tray or top bar.

---

## Privacy & Do Not Disturb Controls

* **Per-App Forwarding Rules**: Choose exactly which apps are allowed to mirror notifications to your desktop (e.g., allow messaging and banking, block games and social media feeds).
* **Privacy on Screen Lock**: Hide notification contents and sender details when your Linux screen is locked.
* **DND (Do Not Disturb) Mirroring**: Putting your laptop into Focus/DND mode automatically silences notifications from your phone, and vice-versa.

---

## Continuum Augmentations (Beyond Apple)

* **Regex Notification Rules**: Mute or highlight notifications based on custom regular expressions (e.g. highlight notifications containing "urgent", auto-dismiss marketing keywords).
* **Scripted Notification Hooks**: Pipe incoming notification text to a shell script (e.g. log IoT sensor alerts to a local file or trigger a desktop sound effect).
* **Notification History Search**: Search past notifications across your mobile and desktop devices locally from your Linux application launcher.
