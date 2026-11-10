# Continuum Documentation Site

This directory contains the official documentation site for **Continuum**, built using [Astro](https://astro.build/) and [Starlight](https://starlight.astro.build/).

## Quick Start

```bash
# Install dependencies
npm install

# Start local development server with hot-reloading
npm run dev

# Build the production static site (output in dist/)
npm run build

# Preview the built site locally
npm run preview
```

## Structure

```
docs/
├── src/
│   └── content/
│       └── docs/
│           ├── index.mdx              # Hero landing page
│           ├── overview/              # Vision, feature matrix, roadmap, superpowers
│           ├── core/                  # Proximity, presence, and identity
│           ├── content/               # Clipboard, AirDrop, Wi-Fi & 2FA sync
│           ├── media/                 # Camera, microphone, media cast receiver
│           ├── workspace/             # Screen mirroring, Sidecar, Universal Control, Sketch
│           └── ecosystem/             # Calls, notifications, biometric PAM, auto-unlock
├── astro.config.mjs                   # Starlight configuration & sidebar navigation
└── package.json
```
