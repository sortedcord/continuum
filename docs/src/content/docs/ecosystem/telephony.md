---
title: Cellular Calls & Telephony Relay
description: Answer, decline, and place phone calls directly on your Linux desktop with full handsfree audio.
---

Continuum Cellular Call Relay bridges your Android phone's cellular carrier connection directly to your Linux desktop or laptop, enabling a full handsfree calling experience without touching your phone.

## User Experience

### 1. Incoming Call Desktop Notification
* When your phone receives an incoming cellular or VoIP call:
  * A prominent, elegant incoming call banner appears on your Linux desktop.
  * Displays the caller’s name, phone number, contact avatar, and carrier label.
  * Provides quick-action buttons:
    * **Answer**: Immediately pick up the call handsfree on your computer.
    * **Decline**: Send the caller to voicemail or drop the call.
    * **Quick Reply**: Send an automated SMS response (e.g. *"In a meeting, will call you back shortly"*).

### 2. Full-Duplex Handsfree Calling
* Once answered on your computer:
  * The call audio routes in real time through your laptop's **microphone and speakers** (or connected Bluetooth/USB headset) using the Linux PipeWire/PulseAudio stack.
  * Your phone remains safely locked in your pocket, bag, or charging dock.
  * Crystal-clear audio quality with full echo cancellation and background noise suppression.

### 3. Active Call Desktop Management
While a call is active, a compact desktop in-call widget gives you complete control:
* **Mute Microphone**: Toggle mic mute with visual status indicator.
* **Audio Device Selector**: Seamlessly switch audio output between laptop speakers, wireless headphones, or external monitors.
* **DTMF Dial Pad**: Access an on-screen numeric keypad to enter digits during automated phone menus (e.g. *"Press 1 for Support"*).
* **Transfer Call to Phone**: One-click handover to resume the call on your physical handset when walking away from your desk.

### 4. Outbound Calling from Desktop
* Click any phone number (`tel:`) on a web page, in an email, or in your desktop contact list to initiate a handsfree call.
* Or open the Continuum desktop dialer, type or paste a phone number, and click **Call**.

---

## Continuum Augmentations (Beyond Apple)

* **Local Call Recording (with Compliance Notification)**: Easily record important business or interview calls directly to a local lossless audio file on your Linux machine with a single click.
* **Headset Auto-Routing**: Automatically route call audio to your dedicated desktop USB microphone and studio headphones rather than generic speakerphone defaults.
* **CLI Dialing Integration**:
  ```bash
  # Trigger an outgoing call from a script or terminal
  continuum call +15551234567

  # Check active call status
  continuum call status
  ```
