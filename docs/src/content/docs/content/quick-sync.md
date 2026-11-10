---
title: Quick Data Sync (Wi-Fi & 2FA)
description: Frictionless Wi-Fi credential sharing and automatic two-factor OTP detection.
---

Continuum eliminates repetitive daily friction points by automatically synchronizing micro-data between your companion devices.

## 1. Wi-Fi Credential Sharing

Joining a complex Wi-Fi network (such as a friend's home or a coffee shop) on a laptop often requires typing long, cryptic passwords.

### User Experience
* When your Linux laptop detects an unknown Wi-Fi network that your nearby paired Android phone is already connected to, a pop-up prompt appears on your phone:
  > *"Share Wi-Fi network 'CoffeeShop_5G' with your Linux Laptop?"*
* Tapping **"Share Password"** transfers the network credentials over the encrypted Continuum link.
* The laptop connects automatically and saves the profile to NetworkManager—zero manual typing required.

---

## 2. SMS 2FA OTP Auto-Fill & Suggestion

Two-factor authentication codes delivered via SMS are a standard security practice, but reading a 6-digit code off a phone screen and manually typing it into a desktop browser is slow and error-prone.

### User Experience
* When an SMS containing a one-time verification code (OTP) arrives on your phone:
  * Continuum automatically detects the 4- to 8-digit verification code and service name (e.g. GitHub, Bank, Google).
  * A discreet, interactive banner appears on your Linux desktop:
    > **GitHub Verification Code: 492-108**
    > `[Copy Code]` `[Auto-Fill]`
  * Clicking **Copy Code** puts the code into your clipboard immediately.
  * In supported browsers, clicking **Auto-Fill** inserts the numbers straight into the active input box.

---

## Privacy & Security Considerations

* **Transient Retention**: OTP codes are automatically purged from memory after 60 seconds to prevent unauthorized reuse.
* **On-Device Regex Detection**: SMS parsing occurs strictly on-device using local regular expressions—no SMS text is ever analyzed by cloud models or external services.
* **Sensitive Network Exclusions**: Enterprise networks requiring internal 802.1X certificates can be configured to opt out of automatic sharing.

---

## Continuum Augmentations (Beyond Apple)

* **Cross-Browser & Terminal Support**: Works across any Linux browser (Firefox, Chromium, Brave, LibreWolf) and can auto-suggest OTPs inside terminal SSH login prompts.
* **Bidirectional Wi-Fi Sync**: If you connect your laptop to a Wi-Fi network via Ethernet/WPS first, you can reverse-share the Wi-Fi credentials to your phone with a single right-click in your desktop network menu.
* **Custom Regex Patterns**: Add custom parsing rules to recognize proprietary banking codes or specialized internal company tokens.
