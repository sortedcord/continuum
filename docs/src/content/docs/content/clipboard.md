---
title: Universal Clipboard
description: Seamless, real-time copy and paste between Linux and Android devices.
---

Universal Clipboard allows you to copy text, formatted content, links, and images on one device and immediately paste them on another device as if they shared a single clipboard.

## User Experience

### 1. Transparent Copy & Paste
* Select text or an image on your Linux computer and press `Ctrl+C`.
* Pick up your Android phone, tap and hold any text box, and tap `Paste`—the exact text or image appears instantly.
* Conversely, copy a tracking number or URL on your phone and press `Ctrl+V` on your Linux machine to paste it immediately.

### 2. Multi-Format Support
* **Plain Text**: Standard strings, code snippets, shell commands, URLs.
* **Rich Text / HTML**: Preserves font styling, headings, links, and tables when pasting between rich text editors and web apps.
* **Images & Screenshots**: Copy a screenshot taken on Linux and paste it straight into a WhatsApp, Signal, or Slack chat on Android (and vice-versa).

### 3. Ambient Privacy Safeguards
* **Transient TTL (Time-to-Live)**: Clipboard entries automatically expire from the shared buffer after a configurable duration (default: 2 minutes) to prevent accidental pastes later.
* **Password Manager Protection**: Clipboard entries marked as sensitive or password-concealed by password managers (e.g., Bitwarden, KeepassXC, 1Password) are automatically excluded from cross-device synchronization.
* **Pause Sync Toggle**: A quick-toggle button in the desktop tray and Android quick settings lets you temporarily pause sync whenever you are working with confidential data.

---

## Continuum Augmentations (Beyond Apple)

* **Multi-Item Clipboard History Buffer**: Apple only keeps the single most recent item. Continuum can optionally maintain a multi-entry clipboard history ring on desktop, letting you paste earlier snippets copied on your phone.
* **CLI Pipe Integration**:
  ```bash
  # Send stdout directly to your phone's clipboard
  git status | continuum clipboard --to phone

  # Read phone's current clipboard into a local file
  continuum clipboard --from phone > phone_notes.txt
  ```
* **Regex Data Transformers**: Automatically clean up links (stripping tracking query parameters like `?utm_source=...`) or format phone numbers during cross-device transit.
* **Selective Device Targeting**: If you have multiple devices nearby (e.g. Work Laptop, Personal Phone, Home Tablet), route clipboard sync specifically to chosen targets or across the entire mesh.
