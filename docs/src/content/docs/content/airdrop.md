---
title: AirDrop Equivalent (High-Speed File Transfer)
description: Zero-configuration, ad-hoc, multi-file and directory peer-to-peer file transfer.
---

Continuum delivers an ultra-fast, zero-configuration file sharing experience between Linux and Android, functioning identically to Apple's AirDrop without requiring third-party cloud drives or messy cable connections.

## User Experience

### 1. Sending from Linux to Android
* **Right-Click Context Menu**: Right-click any file, group of files, or entire directory in your Linux file manager (Nautilus, Dolphin, Thunar, etc.) and click **"Send via Continuum"**.
* **Drag-and-Drop Radar**: An ambient drop zone in your desktop panel displays nearby active devices. Drag any file over your phone's avatar and release to start the transfer immediately.
* **Instant Delivery**: A notification appears on your phone showing a thumbnail preview, file name, and transfer progress. Once complete, files land in your phone's `Downloads` folder or open in the relevant app.

### 2. Sending from Android to Linux
* **System Share Sheet Integration**: Open any photo, video, PDF, or document on your phone, tap the standard Android **Share** button, and select your Linux computer from the direct-share row.
* **Automatic Desktop Acceptance**: If sent from your own paired and trusted phone, files save automatically to your designated Linux folder (e.g. `~/Downloads/Continuum/`) without repetitive prompt fatigue.
* **Guest Acceptance Dialog**: Files received from untrusted guest devices require one-click confirmation with file preview and sender information.

### 3. High-Speed, Resumable Streaming
* Uses the highest-speed available local wireless path (local Wi-Fi or ad-hoc Wi-Fi Direct).
* Transfers multiple gigabytes (such as 4K videos or raw disk images) at maximum hardware bandwidth in seconds.
* Automatically verifies SHA-256 checksums to ensure file integrity.
* If a transfer is temporarily interrupted by moving out of range, it automatically resumes where it left off once the connection re-establishes.

---

## Continuum Augmentations (Beyond Apple)

* **UNIX CLI Piping & Headless Server Support**:
  ```bash
  # Send an entire build folder to your phone
  continuum send ./build/ --to pixel-9

  # Pipe raw command output into a file on your device
  journalctl -u docker | continuum send --name docker.log --to tablet
  ```
* **Folder Watchers (Drop Folders)**: Designate a special folder on your Linux machine (e.g., `~/PhoneDrop`). Any file dropped into this folder is automatically synced to your phone in the background.
* **Multi-Device Broadcast**: Send a photo or presentation simultaneously to multiple nearby colleagues or companion devices in a single transmit pass.
* **Direct Terminal Reception**: Headless Linux home servers or Raspberry Pis can run Continuum as a background daemon, accepting file drops from your phone without needing a desktop environment.
