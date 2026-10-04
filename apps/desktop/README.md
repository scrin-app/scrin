# scrin desktop (Tauri 2)

UI-only shell around `crates/scrin-engine`. The webview runs the same SPA as `apps/web`, built with
`--mode desktop` (sets `VITE_SCRIN_HOST=desktop` → `DesktopHost` from `@scrin/ui/desktop`).

## Run

```powershell
# dev: Vite on :5182 + the shell
pnpm --filter desktop exec tauri dev
# debug exe with the frontend embedded (no installer)
pnpm --filter web exec vite build --mode desktop
cargo build -p scrin-desktop --features custom-protocol
# real Windows capture/encode/input instead of the synthetic test pattern
cargo build -p scrin-desktop --features custom-protocol,win
```

`SCRIN_SERVER=https://…` enables the HTTP resolver for 9-digit IDs; without it, dial by ticket.

## Shape

- `src/bridge.rs`: engine on its own tokio runtime; `call()` with a 5 s deadline; events emitted to
  the webview as `scrin://event` (video frames go to the native presenter instead).
- `src/video.rs`: decoded BGRA → Win32 child window over the webview, D3D11 swap chain
  `FLIP_DISCARD`, latest-frame-wins. The UI reports the canvas rect (`scrin_video_rect`); input over
  the surface goes straight to the engine.
- `src/lib.rs`: `scrin_*` commands, tray (Show / Copy ID / New code / Quit, EN or RO by OS locale),
  close-to-tray, single instance, `scrin://connect/<id>` deep links, autostart (`--hidden`),
  updater, window state.
- `tauri.conf.json`: strict CSP (`default-src 'self'; connect-src ipc: http://ipc.localhost; …`),
  NSIS per-user (English + Romanian) + MSI.
- `capabilities/main.json`: least privilege — only the plugin calls the UI makes.

## Updater key (not set yet)

`plugins.updater.pubkey` is `REPLACE_WITH_MINISIGN_PUBLIC_KEY`. Before the first signed release:

```powershell
pnpm --filter desktop exec tauri signer generate -w "$HOME\.tauri\scrin.key"
```

Put the **public** key in `tauri.conf.json`, keep the private key + password in the release secret
store (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). The endpoint is
`https://github.com/scrin-app/scrin/releases/latest/download/latest.json` (ADR-0011: unsigned
binaries, signed update manifest).
