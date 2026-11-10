---
title: Biometric PAM & Sudo Approval
description: Authorize Linux sudo commands and administrative prompts using your phone's fingerprint sensor.
---

In the Apple ecosystem, an Apple Watch can approve Mac administrator passwords with a quick double-click. Continuum brings this convenience to Linux by allowing your Android phone’s biometric sensor (fingerprint or face unlock) to authorize `sudo` commands and administrative privilege prompts.

## User Experience

### 1. Terminal Sudo Approval
* You type a command requiring root privileges in your Linux terminal:
  ```bash
  sudo systemctl restart nginx
  ```
* Instead of typing your long administrative password:
  * The terminal displays: `[Continuum] Waiting for biometric confirmation on Pixel 9...`
  * A biometric prompt immediately lights up on your nearby phone:
    > **Authorize 'sudo systemctl restart nginx' on Laptop?**
  * Touch your finger to your phone's fingerprint scanner.
  * The prompt clears instantly, and the `sudo` command executes in your terminal.

### 2. Desktop PolicyKit & Keyring Prompts
* When installing software via your package manager or unlocking a saved secret in GNOME Keyring / KDE Wallet:
  * The desktop authorization dialog prompts you for biometric confirmation.
  * A tap on your phone authorizes the action without manual password entry.

---

## Security Architecture

* **Local Challenge-Response**: Every authorization uses an ephemeral, signed cryptographic challenge with a 15-second expiration timeout to prevent replay attacks.
* **Proximity Enforcement**: Biometric approval is only accepted if the phone is verified to be in immediate physical proximity (via low-power radio signal strength), preventing remote attacks if you leave your desk.
* **Graceful Password Fallback**: If your phone battery is dead, out of range, or Bluetooth is disabled, the system instantly falls back to standard Linux password entry with zero delay.

---

## Continuum Augmentations (Beyond Apple)

* **Fine-Grained PAM Policy Rules**: Configure custom policies via `/etc/pam.d/continuum`:
  * Require biometric approval only for specific root commands.
  * Allow biometric approval for desktop logins, screen unlock, and SSH sessions.
* **Smartwatch Biometric Forwarding**: If you wear a WearOS Android smartwatch paired with your phone, approve `sudo` prompts straight from your wrist!
