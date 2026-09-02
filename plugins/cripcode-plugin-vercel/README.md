# CripCode Vercel Plugin

Standalone Vercel hosting plugin for CripCode.

The plugin uses the existing CripCode plugin contract:

- root `plugin.json`
- prebuilt `dist/index.js`
- `toolbar` slot
- host `shell.exec`, `showToast`, and `openTerminal` capabilities

It invokes the Vercel CLI locally. It does not use Ship Studio services or a
Ship Studio plugin registry.

The `repository` field remains empty until this package is published to a
CripCode-owned Git repository. Do not invent or use a Git URL before that repo
exists.
